//! The v0 ⇄ v1 session-image Galois connection.
//!
//! This module is the executable form of `docs/persistence/galois-connection.md`.
//! It realises the two maps between v0 and v1 session images and the laws they
//! satisfy, so that a session keeps its content-addressed identity across a
//! renderer-pipeline upgrade.
//!
//! ```text
//! α : I₁ → I₀   "forget the coefficients"   (lower adjoint)
//! γ : I₀ → I₁   "embed at the semiring top" (upper adjoint)
//! ```
//!
//! * [`SessionImage`](crate::session::SessionImage) is the v0 image (`I₀`): the
//!   base namespace only.
//! * [`SessionImageV1`] is the v1 image (`I₁`): a base image plus exactly one
//!   coefficient per surface.
//! * [`alpha`] drops the coefficient block; [`gamma`] reinstates it at
//!   [`Coefficient::top`].
//!
//! # Identity is base-determined
//!
//! `session_id = "sha256:" + hex(SHA256(C14N(base(doc))))`. The hash is computed
//! over the **base projection only** (`SessionImage::canonical_bytes`), so a v1
//! image and its α share an id — which is what makes the round-trip law hold at
//! the level of identity, not merely structure (L1, identity-stability).
//!
//! As in [`crate::session`], "C14N" here means a deterministic canonicalisation
//! of the *data model* (fixed element order, sorted attributes), not byte-exact
//! W3C C14N. The reference oracle (`reference_check.py`) uses W3C C14N as an
//! oracle for the law *structure*; the hash bytes are an internal choice and are
//! not expected to match the oracle's.
//!
//! # A note on the L2 orientation
//!
//! The spec's order fixes `⊤` (top) as the **greatest**, most-permissive
//! element, with α the lower adjoint and γ the upper adjoint. For a Galois
//! connection in that orientation the round-trip `γ∘α` is *inflationary* —
//! `img₁ ⊑ γ(α(img₁))` — because γ embeds at `⊤` and `⊤` is the maximum. We
//! therefore expose [`SessionImageV1::leq`] and assert the consistent law
//! `img₁ ⊑ γ(α(img₁))` (see tests), alongside the operational statement the
//! oracle checks: `γ(α(img₁))` preserves the base and resets every coefficient
//! to `⊤`. The prose in `galois-connection.md` labels L2 "deflationary" and
//! writes `γ(α(img₁)) ⊑ img₁`; under its own order convention that direction is
//! reversed, so the implementation follows the consistent orientation.

use crate::coefficients::{
    Bound, Coefficient, Confidentiality, Integrity, Interval, Linearity,
};
use crate::session::{
    parse_surface_id, surface_id_to_str, SessionError, SessionImage,
};
use crate::wire::SurfaceId;

/// The XSD namespace for the v1 coefficient block.
pub const COEFFICIENTS_NAMESPACE: &str = "https://simulboot.dev/session/v1/coefficients";

/// The XML prefix used for the coefficient namespace in serialised output.
const COEFF_PREFIX: &str = "c";

/// One surface's coefficient assignment in a v1 image.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceCoefficient {
    /// References a [`SurfaceEntry`](crate::session::SurfaceEntry) by its id.
    pub surface_ref: SurfaceId,
    pub coefficient: Coefficient,
}

/// A v1 session image (`I₁`): a base image plus its per-surface coefficients.
///
/// The invariant (checked by [`SessionImageV1::validate`]) is one coefficient
/// per surface, every `surface_ref` resolving to a surface in `base`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionImageV1 {
    pub base: SessionImage,
    pub coefficients: Vec<SurfaceCoefficient>,
}

impl SessionImageV1 {
    /// The content-addressed id, computed over the base projection only — so it
    /// equals `base.compute_id()` and is invariant under [`alpha`].
    pub fn session_id(&self) -> String {
        self.base.compute_id()
    }

    /// Confirm the structural invariant: exactly one coefficient per surface,
    /// with matching ids.
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.coefficients.len() != self.base.surfaces.len() {
            return Err(SessionError::Malformed(format!(
                "v1 image has {} surfaces but {} coefficients",
                self.base.surfaces.len(),
                self.coefficients.len()
            )));
        }
        for surf in &self.base.surfaces {
            let n = self.coefficients.iter().filter(|c| c.surface_ref == surf.id).count();
            if n != 1 {
                return Err(SessionError::Malformed(format!(
                    "surface {} has {n} coefficients (expected 1)",
                    surface_id_to_str(&surf.id)
                )));
            }
        }
        Ok(())
    }

    /// Look up the coefficient for a surface id.
    pub fn coefficient_for(&self, id: &SurfaceId) -> Option<&Coefficient> {
        self.coefficients.iter().find(|c| &c.surface_ref == id).map(|c| &c.coefficient)
    }

    /// Whether every coefficient is the semiring top — i.e. this image is the
    /// γ-image of a v0 image (`∈ I₀` under the embedding).
    pub fn coeff_is_top(&self) -> bool {
        self.coefficients.iter().all(|c| c.coefficient.is_top())
    }

    /// Bases are equal (under canonical C14N of the base projection).
    pub fn base_eq(&self, other: &SessionImageV1) -> bool {
        self.base.canonical_bytes() == other.base.canonical_bytes()
    }

    /// `self ⊑ other` in `I₁`: equal base, and `coeff(self) ⊑ coeff(other)`
    /// pointwise per surface (the product order of `crate::coefficients`).
    pub fn leq(&self, other: &SessionImageV1) -> bool {
        if !self.base_eq(other) {
            return false;
        }
        // Pointwise on the coefficient dimension, matched by surface id.
        self.coefficients.iter().all(|sc| {
            other
                .coefficient_for(&sc.surface_ref)
                .is_some_and(|o| sc.coefficient.leq(o))
        }) && self.coefficients.len() == other.coefficients.len()
    }

    /// Pretty, human-readable v1 XML with the `id` and an XML declaration.
    pub fn to_xml(&self) -> String {
        let id = self.session_id();
        let extra = self.write_coefficients(true);
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&self.base.write_doc_with_id(true, Some(&id), Some(&extra)));
        out
    }

    /// The canonical, whitespace-free v1 serialisation (base + coefficients),
    /// omitting the `<session>` id attribute. Used to compare v1 documents.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let extra = self.write_coefficients(false);
        self.base.write_doc_with_id(false, None, Some(&extra)).into_bytes()
    }

    /// Parse a v1 image from text XML. The base is parsed by the v0 parser
    /// (which ignores the coefficient namespace — that is α at the parser
    /// level), then the coefficient block is read in a second pass.
    pub fn from_xml(xml: &str) -> Result<SessionImageV1, SessionError> {
        let base = SessionImage::from_xml(xml)?;
        let coefficients = parse_coefficients(xml)?;
        let img = SessionImageV1 { base, coefficients };
        img.validate()?;
        Ok(img)
    }

    /// Render the `<c:coefficients>` block, iterating `base.surfaces` in order so
    /// the output is deterministic and aligned with the surface list.
    fn write_coefficients(&self, pretty: bool) -> String {
        let nl = if pretty { "\n" } else { "" };
        let ind = |n: usize| if pretty { "  ".repeat(n) } else { String::new() };
        let p = COEFF_PREFIX;
        let mut s = String::new();

        s.push_str(&ind(1));
        s.push_str(&format!(
            "<{p}:coefficients xmlns:{p}=\"{COEFFICIENTS_NAMESPACE}\">"
        ));
        s.push_str(nl);

        for surf in &self.base.surfaces {
            let Some(coeff) = self.coefficient_for(&surf.id) else { continue };
            s.push_str(&ind(2));
            s.push_str(&format!(
                "<{p}:surface-coefficient surface-ref=\"{}\">",
                surface_id_to_str(&surf.id)
            ));
            s.push_str(nl);

            // <timing> — one <delay-path> per interval. Attrs sorted: hi, lo.
            s.push_str(&ind(3));
            s.push_str(&format!("<{p}:timing>"));
            s.push_str(nl);
            for iv in &coeff.timing {
                s.push_str(&ind(4));
                s.push_str(&format!(
                    "<{p}:delay-path hi=\"{}\" lo=\"{}\"/>",
                    iv.hi.to_str(),
                    iv.lo.to_str()
                ));
                s.push_str(nl);
            }
            s.push_str(&ind(3));
            s.push_str(&format!("</{p}:timing>"));
            s.push_str(nl);

            // <security> — attrs sorted: confidentiality, integrity.
            s.push_str(&ind(3));
            s.push_str(&format!(
                "<{p}:security confidentiality=\"{}\" integrity=\"{}\"/>",
                coeff.confidentiality.to_str(),
                coeff.integrity.to_str()
            ));
            s.push_str(nl);

            // <linearity>
            s.push_str(&ind(3));
            s.push_str(&format!("<{p}:linearity count=\"{}\"/>", coeff.linearity.to_str()));
            s.push_str(nl);

            s.push_str(&ind(2));
            s.push_str(&format!("</{p}:surface-coefficient>"));
            s.push_str(nl);
        }

        s.push_str(&ind(1));
        s.push_str(&format!("</{p}:coefficients>"));
        s.push_str(nl);
        s
    }
}

/// **α : I₁ → I₀** — abstraction. Forget the coefficients: a v1 image projects
/// to its base, which is a valid v0 image.
pub fn alpha(img: &SessionImageV1) -> SessionImage {
    img.base.clone()
}

/// **γ : I₀ → I₁** — concretisation. Embed a v0 image at the semiring top: one
/// [`Coefficient::top`] per surface. The result satisfies `α(γ(x)) = x`.
pub fn gamma(base: &SessionImage) -> SessionImageV1 {
    let coefficients = base
        .surfaces
        .iter()
        .map(|surf| SurfaceCoefficient { surface_ref: surf.id, coefficient: Coefficient::top() })
        .collect();
    SessionImageV1 { base: base.clone(), coefficients }
}

// --- coefficient-block parsing (the inverse of `write_coefficients`) ---

fn parse_coefficients(xml: &str) -> Result<Vec<SurfaceCoefficient>, SessionError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);

    let mut out: Vec<SurfaceCoefficient> = Vec::new();
    let mut in_coeffs = false;
    let mut cur: Option<CoeffBuilder> = None;

    loop {
        match reader.read_event().map_err(|e| SessionError::Xml(e.to_string()))? {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => match local_name(e.name().as_ref()).as_str() {
                "coefficients" => in_coeffs = true,
                "surface-coefficient" if in_coeffs => {
                    let r = attr(&e, "surface-ref")?;
                    cur = Some(CoeffBuilder::new(parse_surface_id(&r)?));
                }
                "delay-path" if in_coeffs => {
                    let b = cur.as_mut().ok_or_else(|| outside("delay-path"))?;
                    let lo = Bound::parse(&attr(&e, "lo")?)
                        .ok_or_else(|| SessionError::Malformed("bad delay-path/@lo".into()))?;
                    let hi = Bound::parse(&attr(&e, "hi")?)
                        .ok_or_else(|| SessionError::Malformed("bad delay-path/@hi".into()))?;
                    b.timing.push(Interval { lo, hi });
                }
                "security" if in_coeffs => {
                    let b = cur.as_mut().ok_or_else(|| outside("security"))?;
                    b.confidentiality = Some(
                        Confidentiality::parse(&attr(&e, "confidentiality")?).ok_or_else(|| {
                            SessionError::UnknownEnum {
                                kind: "confidentiality",
                                value: attr(&e, "confidentiality").unwrap_or_default(),
                            }
                        })?,
                    );
                    b.integrity = Some(Integrity::parse(&attr(&e, "integrity")?).ok_or_else(
                        || SessionError::UnknownEnum {
                            kind: "integrity",
                            value: attr(&e, "integrity").unwrap_or_default(),
                        },
                    )?);
                }
                "linearity" if in_coeffs => {
                    let b = cur.as_mut().ok_or_else(|| outside("linearity"))?;
                    b.linearity = Some(Linearity::parse(&attr(&e, "count")?).ok_or_else(|| {
                        SessionError::Malformed(format!(
                            "bad linearity/@count {:?}",
                            attr(&e, "count").unwrap_or_default()
                        ))
                    })?);
                }
                _ => {}
            },
            Event::End(e) => match local_name(e.name().as_ref()).as_str() {
                "coefficients" => in_coeffs = false,
                "surface-coefficient" => {
                    if let Some(b) = cur.take() {
                        out.push(b.build()?);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(out)
}

struct CoeffBuilder {
    surface_ref: SurfaceId,
    timing: Vec<Interval>,
    confidentiality: Option<Confidentiality>,
    integrity: Option<Integrity>,
    linearity: Option<Linearity>,
}

impl CoeffBuilder {
    fn new(surface_ref: SurfaceId) -> Self {
        CoeffBuilder {
            surface_ref,
            timing: Vec::new(),
            confidentiality: None,
            integrity: None,
            linearity: None,
        }
    }

    fn build(self) -> Result<SurfaceCoefficient, SessionError> {
        let miss = |f: &str| SessionError::Malformed(format!("surface-coefficient missing <{f}>"));
        if self.timing.is_empty() {
            return Err(miss("timing/delay-path"));
        }
        Ok(SurfaceCoefficient {
            surface_ref: self.surface_ref,
            coefficient: Coefficient {
                timing: self.timing,
                confidentiality: self.confidentiality.ok_or_else(|| miss("security"))?,
                integrity: self.integrity.ok_or_else(|| miss("security"))?,
                linearity: self.linearity.ok_or_else(|| miss("linearity"))?,
            },
        })
    }
}

fn outside(tag: &str) -> SessionError {
    SessionError::Malformed(format!("<{tag}> outside <surface-coefficient>"))
}

fn local_name(qname: &[u8]) -> String {
    let s = String::from_utf8_lossy(qname);
    match s.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => s.into_owned(),
    }
}

fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Result<String, SessionError> {
    for a in e.attributes() {
        let a = a.map_err(|e| SessionError::Xml(e.to_string()))?;
        if local_name(a.key.as_ref()) == key {
            return Ok(a
                .unescape_value()
                .map_err(|e| SessionError::Xml(e.to_string()))?
                .into_owned());
        }
    }
    Err(SessionError::Malformed(format!("missing attribute @{key}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{HostEntry, Layout, SurfaceEntry};
    use crate::wire::{Codec, OsKind};

    fn sid(b: u8) -> SurfaceId {
        [b; 32]
    }

    /// A concrete v0 image mirroring `reference_check.py`'s `V0` (with real
    /// 32-byte ids in place of the doc's `sha256:aaa` placeholders).
    fn img0() -> SessionImage {
        let surfaces = vec![
            SurfaceEntry {
                id: sid(0xaa),
                name: "macOS".into(),
                order: 0,
                host: HostEntry {
                    address: "127.0.0.1:7001".into(),
                    os: OsKind::MacOS,
                    machine: "Aleth-MacBook".into(),
                    capture: "window:Safari".into(),
                },
                codec: Codec::H265,
                width: 960,
                height: 540,
            },
            SurfaceEntry {
                id: sid(0xbb),
                name: "Windows".into(),
                order: 1,
                host: HostEntry {
                    address: "100.1.1.2:7001".into(),
                    os: OsKind::Windows,
                    machine: "Aleth-PC".into(),
                    capture: "display:0".into(),
                },
                codec: Codec::H265,
                width: 960,
                height: 540,
            },
        ];
        SessionImage::new(
            "2026-06-28T19:44:09Z",
            surfaces,
            Layout { scroll_pos: 0.0, focus: Some(sid(0xaa)) },
        )
    }

    /// A non-top coefficient for L2 / order tests.
    fn constrained() -> Coefficient {
        Coefficient {
            timing: vec![Interval { lo: Bound::Finite(0.0), hi: Bound::Finite(16_000_000.0) }],
            confidentiality: Confidentiality::Secret,
            integrity: Integrity::Trusted,
            linearity: Linearity::Finite(1),
        }
    }

    // ---- L1: v0 round-trip is the identity (lossless forward compatibility) ----

    #[test]
    fn l1_alpha_gamma_is_identity() {
        let img0 = img0();
        let back = alpha(&gamma(&img0));
        assert_eq!(back, img0);
        // And byte-identical after canonicalisation → same content id.
        assert_eq!(back.canonical_bytes(), img0.canonical_bytes());
        assert_eq!(back.compute_id(), img0.compute_id());
    }

    // ---- identity-stability: the hash is invariant under abstraction ----

    #[test]
    fn identity_stable_under_abstraction() {
        let img0 = img0();
        let img1 = gamma(&img0);
        assert_eq!(img1.session_id(), img0.compute_id());
        assert_eq!(alpha(&img1).compute_id(), img1.session_id());
    }

    // ---- L2: v1 round-trip preserves base and resets coefficients to top ----

    #[test]
    fn l2_operational_resets_to_top() {
        let img0 = img0();
        // A genuinely non-top v1 image.
        let mut img1 = gamma(&img0);
        img1.coefficients[0].coefficient = constrained();
        assert!(!img1.coeff_is_top());

        let ga = gamma(&alpha(&img1));
        // base preserved …
        assert!(ga.base_eq(&img1));
        // … and coefficients reset to top.
        assert!(ga.coeff_is_top());
    }

    #[test]
    fn l2_order_is_inflationary_to_top() {
        // Under the spec's order (top = greatest), γ∘α inflates: img₁ ⊑ γ(α(img₁)),
        // with equality iff img₁ was already all-top.
        let img0 = img0();
        let mut img1 = gamma(&img0);
        img1.coefficients[0].coefficient = constrained();

        let ga = gamma(&alpha(&img1));
        assert!(img1.leq(&ga), "img1 should be below its top-reset round-trip");
        assert!(!ga.leq(&img1), "the round-trip strictly dominates a non-top image");

        // Equality case: an already-top image round-trips to itself.
        let top1 = gamma(&img0);
        assert!(top1.leq(&gamma(&alpha(&top1))));
        assert!(gamma(&alpha(&top1)).leq(&top1));
        assert_eq!(gamma(&alpha(&top1)), top1);
    }

    // ---- The Galois connection law itself ----

    #[test]
    fn galois_connection_law() {
        // α(x) ⊑ y  ⟺  x ⊑ γ(y)     for x ∈ I₁, y ∈ I₀.
        let img0 = img0();
        let mut x = gamma(&img0);
        x.coefficients[1].coefficient = constrained();

        // y with the same base ⇒ both sides hold.
        let y_same = img0.clone();
        let lhs = alpha(&x).canonical_bytes() == y_same.canonical_bytes();
        let rhs = x.leq(&gamma(&y_same));
        assert!(lhs && rhs);
        assert_eq!(lhs, rhs);

        // y with a different base ⇒ both sides fail.
        let mut other = img0.clone();
        other.surfaces[0].name = "different".into();
        let lhs2 = alpha(&x).canonical_bytes() == other.canonical_bytes();
        let rhs2 = x.leq(&gamma(&other));
        assert!(!lhs2 && !rhs2);
        assert_eq!(lhs2, rhs2);
    }

    // ---- L3: renderer-migration law with an identity-stub v1 renderer ----

    #[test]
    fn l3_migration_with_identity_stub() {
        // Stub load_v1 / checkpoint_v1 that preserve coefficients (the
        // per-renderer obligation, as a fixture). Then:
        //   α(checkpoint_v1(load_v1(γ(img0)))) == img0.
        let load_v1 = |x: SessionImageV1| x;
        let checkpoint_v1 = |x: SessionImageV1| x;

        let img0 = img0();
        let migrated = alpha(&checkpoint_v1(load_v1(gamma(&img0))));
        assert_eq!(migrated, img0);
        assert_eq!(migrated.compute_id(), img0.compute_id());
    }

    // ---- XML carrier: round-trips and parser-level α ----

    #[test]
    fn v1_xml_roundtrips() {
        // to_xml writes the (base-determined) id, which from_xml reads back into
        // base.id; build the fixture with that id populated so PartialEq lines up.
        let mut img1 = gamma(&img0().with_computed_id());
        img1.coefficients[0].coefficient = constrained();
        let xml = img1.to_xml();
        let parsed = SessionImageV1::from_xml(&xml).unwrap();
        assert_eq!(parsed, img1);
    }

    #[test]
    fn gamma_inserts_top_in_xml() {
        let xml = gamma(&img0()).to_xml();
        assert!(xml.contains("lo=\"-INF\""));
        assert!(xml.contains("hi=\"+INF\""));
        assert!(xml.contains("confidentiality=\"public\""));
        assert!(xml.contains("integrity=\"untrusted\""));
        assert!(xml.contains("count=\"omega\""));
        assert!(xml.contains(COEFFICIENTS_NAMESPACE));
    }

    #[test]
    fn v0_parser_ignores_coefficients() {
        // A v0 parser reading a v1 document recovers exactly the base — this is
        // α at the parser level (forward compatibility via the namespace split).
        let img0 = img0();
        let v1_xml = gamma(&img0).to_xml();
        let base_from_v1 = SessionImage::from_xml(&v1_xml).unwrap();
        // Same content (id is base-determined and recomputed on parse).
        assert_eq!(base_from_v1.canonical_bytes(), img0.canonical_bytes());
        assert_eq!(base_from_v1.compute_id(), img0.compute_id());
    }

    #[test]
    fn validate_rejects_surface_count_mismatch() {
        let mut img1 = gamma(&img0());
        img1.coefficients.pop();
        assert!(img1.validate().is_err());
    }
}
