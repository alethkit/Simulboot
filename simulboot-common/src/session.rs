//! The session image: a self-describing, content-addressed snapshot of a
//! running session.
//!
//! A [`SessionImage`] is the thing that makes the session separable from any OS
//! instance (Claim A). It records every surface, that surface's provenance and
//! host address, and the strip layout (order, scroll position, focus). A bare
//! compositor can load one and fully reconstitute the session with no external
//! config (F6).
//!
//! # Format
//!
//! The abstract model is an XML Information Set in the
//! `https://simulboot.dev/session/v1` namespace. Three concrete forms exist in
//! the brief; v0 implements the two text ones:
//!
//! * **Text XML** ([`SessionImage::to_xml`]) — human-readable, indented, with an
//!   XML declaration. For debugging and version control.
//! * **Canonical form** ([`SessionImage::canonical_bytes`]) — a deterministic,
//!   whitespace-free serialisation of the data model used for content
//!   addressing.
//!
//! (Fast Infoset, the binary form, is out of scope for v0.)
//!
//! # Content addressing
//!
//! `session_id = "sha256:" + hex(SHA256(canonical_bytes))`. To avoid the obvious
//! circularity — the id can't be an input to the hash that produces it — the
//! canonical form omits the `id` attribute on `<session>`. Real W3C C14N
//! canonicalises arbitrary serialised XML; here we canonicalise the *data model*
//! directly with a fixed element order and attributes sorted by name, which is
//! sufficient and far simpler for v0.

use crate::wire::{Codec, OsKind, SurfaceId};
use sha2::{Digest, Sha256};

/// The XSD namespace for v1 session images.
pub const SESSION_NAMESPACE: &str = "https://simulboot.dev/session/v1";

/// Current schema version written into every image.
pub const SCHEMA_VERSION: u32 = 1;

/// Errors from building, serialising, or parsing a session image.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("malformed session image: {0}")]
    Malformed(String),
    #[error("unknown {kind} value: {value:?}")]
    UnknownEnum { kind: &'static str, value: String },
    #[error("bad surface id {value:?}: {reason}")]
    BadSurfaceId { value: String, reason: String },
    /// The recomputed content hash does not match the declared id.
    #[error("integrity check failed: declared {declared}, computed {computed}")]
    IntegrityMismatch { declared: String, computed: String },
}

/// A host endpoint plus the provenance of what it captures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEntry {
    /// Socket address the compositor reconnects to, e.g. `100.x.x.y:7001`.
    pub address: String,
    pub os: OsKind,
    pub machine: String,
    /// e.g. `window:Safari`, `display:0`, `vm:PhysicalDrive1`.
    pub capture: String,
}

/// One surface as recorded in the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceEntry {
    pub id: SurfaceId,
    pub name: String,
    /// Position in the strip, left to right, starting at 0.
    pub order: u32,
    pub host: HostEntry,
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
}

/// Strip layout state: scroll position and which surface (if any) has focus.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Pixels scrolled from the left edge of the infinite strip.
    pub scroll_pos: f32,
    /// The focused surface, if any.
    pub focus: Option<SurfaceId>,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { scroll_pos: 0.0, focus: None }
    }
}

/// A complete session image.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionImage {
    /// The content-addressed id (`sha256:...`). `None` until computed or parsed;
    /// [`to_xml`](Self::to_xml) fills it in on output.
    pub id: Option<String>,
    /// Creation timestamp, RFC 3339. Supplied by the caller (this crate is
    /// clock-free so it stays deterministic and dependency-light).
    pub created: String,
    pub schema_version: u32,
    pub surfaces: Vec<SurfaceEntry>,
    pub layout: Layout,
}

impl SessionImage {
    /// Build an image from surfaces and layout. `created` is an RFC 3339 string
    /// the caller supplies (e.g. from `time`/`chrono` in the compositor). The id
    /// is left unset; call [`with_computed_id`](Self::with_computed_id) or
    /// [`to_xml`](Self::to_xml) to populate it.
    pub fn new(created: impl Into<String>, surfaces: Vec<SurfaceEntry>, layout: Layout) -> Self {
        SessionImage {
            id: None,
            created: created.into(),
            schema_version: SCHEMA_VERSION,
            surfaces,
            layout,
        }
    }

    /// The content-addressed id of this image: `sha256:` followed by the hex
    /// SHA-256 of the canonical form. Independent of the current `id` field.
    pub fn compute_id(&self) -> String {
        let digest = Sha256::digest(self.canonical_bytes());
        format!("sha256:{}", hex::encode(digest))
    }

    /// Return a copy with `id` set to the computed content hash.
    pub fn with_computed_id(mut self) -> Self {
        self.id = Some(self.compute_id());
        self
    }

    /// Recompute the content hash and confirm it matches `self.id`. Used on
    /// resume to verify a fetched image has not been tampered with.
    pub fn verify(&self) -> Result<(), SessionError> {
        let computed = self.compute_id();
        match &self.id {
            Some(declared) if declared == &computed => Ok(()),
            Some(declared) => Err(SessionError::IntegrityMismatch {
                declared: declared.clone(),
                computed,
            }),
            None => Err(SessionError::Malformed("no id to verify against".into())),
        }
    }

    /// The canonical, deterministic, whitespace-free serialisation used for
    /// content addressing. Omits the XML declaration and the `<session>` `id`
    /// attribute; attributes within each element are written in sorted order.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        self.write_doc(false, false).into_bytes()
    }

    /// Human-readable, indented text XML including the computed `id` and an XML
    /// declaration. This is what gets written to disk for debugging.
    pub fn to_xml(&self) -> String {
        let id = self.id.clone().unwrap_or_else(|| self.compute_id());
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&self.write_doc_with_id(true, Some(&id), None));
        out
    }

    /// Parse a session image from text XML. Captures the declared `id` so it can
    /// later be checked with [`verify`](Self::verify).
    pub fn from_xml(xml: &str) -> Result<Self, SessionError> {
        parse::from_xml(xml)
    }

    // --- serialisation internals ---

    fn write_doc(&self, pretty: bool, include_id: bool) -> String {
        let id = if include_id { self.id.clone() } else { None };
        self.write_doc_with_id(pretty, id.as_deref(), None)
    }

    /// Serialise the `<session>` document. `extra_children`, if present, is a
    /// pre-rendered block of additional child elements (e.g. the v1
    /// coefficients namespace, supplied by [`crate::galois`]) spliced in after
    /// `<layout>` and before `</session>`. It must already be indented for
    /// `pretty` and carry its own trailing newline when pretty.
    pub(crate) fn write_doc_with_id(
        &self,
        pretty: bool,
        id: Option<&str>,
        extra_children: Option<&str>,
    ) -> String {
        let nl = if pretty { "\n" } else { "" };
        let ind = |n: usize| if pretty { "  ".repeat(n) } else { String::new() };
        let mut s = String::new();

        // <session> — namespace decl first, then attributes in sorted order
        // (created, id, schema-version). id is omitted from the canonical form.
        s.push_str("<session xmlns=\"");
        s.push_str(SESSION_NAMESPACE);
        s.push('"');
        s.push_str(&format!(" created=\"{}\"", escape_attr(&self.created)));
        if let Some(id) = id {
            s.push_str(&format!(" id=\"{}\"", escape_attr(id)));
        }
        s.push_str(&format!(" schema-version=\"{}\"", self.schema_version));
        s.push('>');
        s.push_str(nl);

        // <surfaces>
        s.push_str(&ind(1));
        s.push_str("<surfaces>");
        s.push_str(nl);
        for surf in &self.surfaces {
            write_surface(&mut s, surf, pretty, &ind);
        }
        s.push_str(&ind(1));
        s.push_str("</surfaces>");
        s.push_str(nl);

        // <layout>
        s.push_str(&ind(1));
        s.push_str("<layout>");
        s.push_str(nl);
        s.push_str(&ind(2));
        s.push_str(&format!("<strip scroll-pos=\"{}\"/>", fmt_f32(self.layout.scroll_pos)));
        s.push_str(nl);
        if let Some(focus) = &self.layout.focus {
            s.push_str(&ind(2));
            s.push_str(&format!(
                "<focus surface-ref=\"{}\"/>",
                escape_attr(&surface_id_to_str(focus))
            ));
            s.push_str(nl);
        }
        s.push_str(&ind(1));
        s.push_str("</layout>");
        s.push_str(nl);

        // v1 coefficients (a sibling of <surfaces>/<layout>, in its own
        // namespace) splice in here; α is exactly "drop this block".
        if let Some(extra) = extra_children {
            s.push_str(extra);
        }

        s.push_str("</session>");
        s
    }
}

fn write_surface(s: &mut String, surf: &SurfaceEntry, pretty: bool, ind: &dyn Fn(usize) -> String) {
    let nl = if pretty { "\n" } else { "" };
    // <surface> attributes sorted: id, name, order.
    s.push_str(&ind(2));
    s.push_str(&format!(
        "<surface id=\"{}\" name=\"{}\" order=\"{}\">",
        escape_attr(&surface_id_to_str(&surf.id)),
        escape_attr(&surf.name),
        surf.order
    ));
    s.push_str(nl);

    // <host>
    s.push_str(&ind(3));
    s.push_str("<host>");
    s.push_str(nl);
    write_text_el(s, 4, "address", &surf.host.address, pretty, ind);
    write_text_el(s, 4, "os", os_to_str(surf.host.os), pretty, ind);
    write_text_el(s, 4, "machine", &surf.host.machine, pretty, ind);
    write_text_el(s, 4, "capture", &surf.host.capture, pretty, ind);
    s.push_str(&ind(3));
    s.push_str("</host>");
    s.push_str(nl);

    // <codec>
    write_text_el(s, 3, "codec", codec_to_str(surf.codec), pretty, ind);

    // <dimensions> attributes sorted: height, width.
    s.push_str(&ind(3));
    s.push_str(&format!(
        "<dimensions height=\"{}\" width=\"{}\"/>",
        surf.height, surf.width
    ));
    s.push_str(nl);

    s.push_str(&ind(2));
    s.push_str("</surface>");
    s.push_str(nl);
}

fn write_text_el(
    s: &mut String,
    depth: usize,
    tag: &str,
    text: &str,
    pretty: bool,
    ind: &dyn Fn(usize) -> String,
) {
    let nl = if pretty { "\n" } else { "" };
    s.push_str(&ind(depth));
    s.push_str(&format!("<{tag}>{}</{tag}>", escape_text(text)));
    s.push_str(nl);
}

// --- enum <-> string mappings (the on-the-wire XML spellings) ---

fn os_to_str(os: OsKind) -> &'static str {
    match os {
        OsKind::MacOS => "macOS",
        OsKind::Windows => "Windows",
        OsKind::Linux => "Linux",
    }
}

fn os_from_str(s: &str) -> Result<OsKind, SessionError> {
    match s {
        "macOS" => Ok(OsKind::MacOS),
        "Windows" => Ok(OsKind::Windows),
        "Linux" => Ok(OsKind::Linux),
        other => Err(SessionError::UnknownEnum { kind: "os", value: other.to_string() }),
    }
}

fn codec_to_str(c: Codec) -> &'static str {
    match c {
        Codec::H265 => "H265",
        Codec::AV1 => "AV1",
    }
}

fn codec_from_str(s: &str) -> Result<Codec, SessionError> {
    match s {
        "H265" => Ok(Codec::H265),
        "AV1" => Ok(Codec::AV1),
        other => Err(SessionError::UnknownEnum { kind: "codec", value: other.to_string() }),
    }
}

/// Render a [`SurfaceId`] as `sha256:<64 hex chars>`.
pub fn surface_id_to_str(id: &SurfaceId) -> String {
    format!("sha256:{}", hex::encode(id))
}

/// Parse a `sha256:<64 hex chars>` string back into a [`SurfaceId`].
pub fn parse_surface_id(s: &str) -> Result<SurfaceId, SessionError> {
    let hex_part = s.strip_prefix("sha256:").ok_or_else(|| SessionError::BadSurfaceId {
        value: s.to_string(),
        reason: "missing 'sha256:' prefix".into(),
    })?;
    let bytes = hex::decode(hex_part).map_err(|e| SessionError::BadSurfaceId {
        value: s.to_string(),
        reason: e.to_string(),
    })?;
    bytes.try_into().map_err(|v: Vec<u8>| SessionError::BadSurfaceId {
        value: s.to_string(),
        reason: format!("expected 32 bytes, got {}", v.len()),
    })
}

/// Deterministic, round-trippable float formatting. Rust's default `{}` already
/// produces the shortest representation that round-trips, which is exactly what
/// canonical addressing needs.
fn fmt_f32(v: f32) -> String {
    // Normalise -0.0 to 0 so the canonical form is stable.
    if v == 0.0 {
        "0".to_string()
    } else {
        format!("{v}")
    }
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

// --- parsing ---

mod parse {
    use super::*;
    use quick_xml::events::Event;
    use quick_xml::Reader;

    /// A tiny element model is overkill; we walk events and accumulate into
    /// builders keyed by the element we're currently inside.
    pub(super) fn from_xml(xml: &str) -> Result<SessionImage, SessionError> {
        let mut reader = Reader::from_str(xml);
        reader.trim_text(true);

        let mut session_attrs = SessionAttrs::default();
        let mut surfaces: Vec<SurfaceEntry> = Vec::new();
        let mut layout = Layout::default();

        // In-progress surface/host state.
        let mut cur_surface: Option<SurfaceBuilder> = None;
        let mut in_host = false;
        let mut text_target: Option<TextTarget> = None;
        let mut text_buf = String::new();

        loop {
            match reader.read_event().map_err(|e| SessionError::Xml(e.to_string()))? {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) => {
                    let name = local_name(e.name().as_ref());
                    match name.as_str() {
                        "session" => session_attrs = SessionAttrs::parse(&e)?,
                        "surface" => cur_surface = Some(SurfaceBuilder::parse(&e)?),
                        "host" => in_host = true,
                        "address" => text_target = Some(TextTarget::Address),
                        "os" => text_target = Some(TextTarget::Os),
                        "machine" => text_target = Some(TextTarget::Machine),
                        "capture" => text_target = Some(TextTarget::Capture),
                        "codec" => text_target = Some(TextTarget::Codec),
                        "dimensions" => {
                            let b = cur_surface
                                .as_mut()
                                .ok_or_else(|| SessionError::Malformed("<dimensions> outside <surface>".into()))?;
                            b.width = Some(attr_u32(&e, "width")?);
                            b.height = Some(attr_u32(&e, "height")?);
                        }
                        "strip" => layout.scroll_pos = attr_f32(&e, "scroll-pos")?,
                        "focus" => {
                            let r = attr_str(&e, "surface-ref")?;
                            layout.focus = Some(parse_surface_id(&r)?);
                        }
                        _ => {}
                    }
                    text_buf.clear();
                }
                Event::Text(t) => {
                    if text_target.is_some() {
                        text_buf.push_str(&t.unescape().map_err(|e| SessionError::Xml(e.to_string()))?);
                    }
                }
                Event::End(e) => {
                    let name = local_name(e.name().as_ref());
                    match name.as_str() {
                        "host" => in_host = false,
                        "surface" => {
                            let b = cur_surface
                                .take()
                                .ok_or_else(|| SessionError::Malformed("</surface> without start".into()))?;
                            surfaces.push(b.build()?);
                        }
                        "address" | "os" | "machine" | "capture" | "codec" => {
                            apply_text(&mut cur_surface, in_host, &text_target, &text_buf)?;
                            text_target = None;
                            text_buf.clear();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        let created = session_attrs
            .created
            .ok_or_else(|| SessionError::Malformed("missing session/@created".into()))?;
        surfaces.sort_by_key(|s| s.order);

        Ok(SessionImage {
            id: session_attrs.id,
            created,
            schema_version: session_attrs.schema_version.unwrap_or(SCHEMA_VERSION),
            surfaces,
            layout,
        })
    }

    fn apply_text(
        cur: &mut Option<SurfaceBuilder>,
        in_host: bool,
        target: &Option<TextTarget>,
        text: &str,
    ) -> Result<(), SessionError> {
        let b = cur
            .as_mut()
            .ok_or_else(|| SessionError::Malformed("text element outside <surface>".into()))?;
        match target {
            Some(TextTarget::Address) if in_host => b.address = Some(text.to_string()),
            Some(TextTarget::Os) if in_host => b.os = Some(os_from_str(text)?),
            Some(TextTarget::Machine) if in_host => b.machine = Some(text.to_string()),
            Some(TextTarget::Capture) if in_host => b.capture = Some(text.to_string()),
            Some(TextTarget::Codec) => b.codec = Some(codec_from_str(text)?),
            _ => {}
        }
        Ok(())
    }

    enum TextTarget {
        Address,
        Os,
        Machine,
        Capture,
        Codec,
    }

    #[derive(Default)]
    struct SessionAttrs {
        id: Option<String>,
        created: Option<String>,
        schema_version: Option<u32>,
    }

    impl SessionAttrs {
        fn parse(e: &quick_xml::events::BytesStart) -> Result<Self, SessionError> {
            let mut out = SessionAttrs::default();
            for attr in e.attributes() {
                let attr = attr.map_err(|e| SessionError::Xml(e.to_string()))?;
                let key = local_name(attr.key.as_ref());
                let val = attr
                    .unescape_value()
                    .map_err(|e| SessionError::Xml(e.to_string()))?
                    .into_owned();
                match key.as_str() {
                    "id" => out.id = Some(val),
                    "created" => out.created = Some(val),
                    "schema-version" => {
                        out.schema_version = Some(val.parse().map_err(|_| {
                            SessionError::Malformed(format!("bad schema-version {val:?}"))
                        })?)
                    }
                    _ => {}
                }
            }
            Ok(out)
        }
    }

    struct SurfaceBuilder {
        id: SurfaceId,
        name: String,
        order: u32,
        address: Option<String>,
        os: Option<OsKind>,
        machine: Option<String>,
        capture: Option<String>,
        codec: Option<Codec>,
        width: Option<u32>,
        height: Option<u32>,
    }

    impl SurfaceBuilder {
        fn parse(e: &quick_xml::events::BytesStart) -> Result<Self, SessionError> {
            Ok(SurfaceBuilder {
                id: parse_surface_id(&attr_str(e, "id")?)?,
                name: attr_str(e, "name")?,
                order: attr_u32(e, "order")?,
                address: None,
                os: None,
                machine: None,
                capture: None,
                codec: None,
                width: None,
                height: None,
            })
        }

        fn build(self) -> Result<SurfaceEntry, SessionError> {
            let miss = |f: &str| SessionError::Malformed(format!("surface missing <{f}>"));
            Ok(SurfaceEntry {
                id: self.id,
                name: self.name,
                order: self.order,
                host: HostEntry {
                    address: self.address.ok_or_else(|| miss("address"))?,
                    os: self.os.ok_or_else(|| miss("os"))?,
                    machine: self.machine.ok_or_else(|| miss("machine"))?,
                    capture: self.capture.ok_or_else(|| miss("capture"))?,
                },
                codec: self.codec.ok_or_else(|| miss("codec"))?,
                width: self.width.ok_or_else(|| miss("dimensions/@width"))?,
                height: self.height.ok_or_else(|| miss("dimensions/@height"))?,
            })
        }
    }

    fn local_name(qname: &[u8]) -> String {
        let s = String::from_utf8_lossy(qname);
        match s.rsplit_once(':') {
            Some((_, local)) => local.to_string(),
            None => s.into_owned(),
        }
    }

    fn attr_str(e: &quick_xml::events::BytesStart, key: &str) -> Result<String, SessionError> {
        for attr in e.attributes() {
            let attr = attr.map_err(|e| SessionError::Xml(e.to_string()))?;
            if local_name(attr.key.as_ref()) == key {
                return Ok(attr
                    .unescape_value()
                    .map_err(|e| SessionError::Xml(e.to_string()))?
                    .into_owned());
            }
        }
        Err(SessionError::Malformed(format!("missing attribute @{key}")))
    }

    fn attr_u32(e: &quick_xml::events::BytesStart, key: &str) -> Result<u32, SessionError> {
        let v = attr_str(e, key)?;
        v.parse()
            .map_err(|_| SessionError::Malformed(format!("attribute @{key} is not a u32: {v:?}")))
    }

    fn attr_f32(e: &quick_xml::events::BytesStart, key: &str) -> Result<f32, SessionError> {
        let v = attr_str(e, key)?;
        v.parse()
            .map_err(|_| SessionError::Malformed(format!("attribute @{key} is not an f32: {v:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Codec, OsKind};

    fn sid(byte: u8) -> SurfaceId {
        [byte; 32]
    }

    fn sample() -> SessionImage {
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
                    address: "100.0.0.2:7001".into(),
                    os: OsKind::Windows,
                    machine: "Aleth-PC".into(),
                    capture: "display:0".into(),
                },
                codec: Codec::H265,
                width: 960,
                height: 540,
            },
        ];
        let layout = Layout { scroll_pos: 0.0, focus: Some(sid(0xaa)) };
        SessionImage::new("2026-06-28T19:44:09Z", surfaces, layout)
    }

    #[test]
    fn roundtrip_preserves_data() {
        let original = sample().with_computed_id();
        let xml = original.to_xml();
        let parsed = SessionImage::from_xml(&xml).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn id_is_stable_across_serialisation() {
        let img = sample();
        let id1 = img.compute_id();
        let xml = img.to_xml();
        let parsed = SessionImage::from_xml(&xml).unwrap();
        assert_eq!(parsed.compute_id(), id1);
        assert_eq!(parsed.id.as_deref(), Some(id1.as_str()));
    }

    #[test]
    fn verify_accepts_untampered_image() {
        let img = sample().with_computed_id();
        img.verify().unwrap();
    }

    #[test]
    fn verify_rejects_tampered_image() {
        let mut img = sample().with_computed_id();
        // Mutate content without recomputing the id.
        img.surfaces[0].name = "tampered".into();
        let err = img.verify().unwrap_err();
        assert!(matches!(err, SessionError::IntegrityMismatch { .. }));
    }

    #[test]
    fn canonical_form_omits_id_and_declaration() {
        let img = sample().with_computed_id();
        let canon = String::from_utf8(img.canonical_bytes()).unwrap();
        assert!(!canon.contains("<?xml"));
        // The <session> element itself carries no id attribute (that would be
        // circular); surfaces still do.
        let session_tag = &canon[..canon.find('>').unwrap()];
        assert!(!session_tag.contains(" id="), "session tag was: {session_tag}");
        assert!(session_tag.contains("schema-version="));
        assert!(canon.contains(SESSION_NAMESPACE));
    }

    #[test]
    fn focusless_layout_roundtrips() {
        let mut img = sample();
        img.layout.focus = None;
        let img = img.with_computed_id();
        let parsed = SessionImage::from_xml(&img.to_xml()).unwrap();
        assert_eq!(parsed.layout.focus, None);
        assert_eq!(parsed, img);
    }

    #[test]
    fn surface_id_string_roundtrips() {
        let id = sid(0x3c);
        let s = surface_id_to_str(&id);
        assert_eq!(parse_surface_id(&s).unwrap(), id);
    }
}
