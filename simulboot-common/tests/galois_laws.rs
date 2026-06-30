//! Property tests for the v0 ⇄ v1 persistence Galois connection.
//!
//! These check the laws of `docs/persistence/galois-connection.md` over randomly
//! generated session images, against the public API only. They are pure
//! XML/hash/order properties — no rendering, no GPU, no network — and are
//! runnable in v0 before any v1 renderer exists, exactly as the spec intends.
//!
//! The generators follow the "Generator for property tests (shape)" section of
//! the spec: an `I₀` strategy (v0 images) and an `I₁` strategy (v0 + a random,
//! possibly non-top coefficient per surface).

use proptest::prelude::*;

use simulboot_common::coefficients::{
    Bound, Coefficient, Confidentiality, Integrity, Interval, Linearity,
};
use simulboot_common::galois::{alpha, gamma, SessionImageV1, SurfaceCoefficient};
use simulboot_common::session::{HostEntry, Layout, SessionImage, SurfaceEntry};
use simulboot_common::wire::{Codec, OsKind};

// ---- generators (the `I₀` / `I₁` strategies) ----

fn arb_os() -> impl Strategy<Value = OsKind> {
    prop_oneof![Just(OsKind::MacOS), Just(OsKind::Windows), Just(OsKind::Linux)]
}

fn arb_codec() -> impl Strategy<Value = Codec> {
    prop_oneof![Just(Codec::H265), Just(Codec::AV1)]
}

fn arb_dims() -> impl Strategy<Value = (u32, u32)> {
    prop_oneof![Just((960u32, 540u32)), Just((1280, 720)), Just((1920, 1080)), Just((640, 480))]
}

fn arb_address() -> impl Strategy<Value = String> {
    // Tailnet-shaped: 100.x.y.z:7001.
    (0u8..=255, 0u8..=255, 0u8..=255).prop_map(|(x, y, z)| format!("100.{x}.{y}.{z}:7001"))
}

/// One surface at position `order`, with a content-ish random id.
fn arb_surface(order: u32) -> impl Strategy<Value = SurfaceEntry> {
    (
        proptest::array::uniform32(any::<u8>()),
        prop_oneof![Just("macOS"), Just("Windows"), Just("Linux"), Just("VM")],
        arb_os(),
        "[a-z]{3,8}",
        prop_oneof![Just("display:0"), Just("window:Safari"), Just("vm:Disk1")],
        arb_address(),
        arb_codec(),
        arb_dims(),
    )
        .prop_map(move |(id, name, os, machine, capture, address, codec, (w, h))| SurfaceEntry {
            id,
            name: name.to_string(),
            order,
            host: HostEntry { address, os, machine, capture: capture.to_string() },
            codec,
            width: w,
            height: h,
        })
}

/// `I₀`: a valid v0 image with 1–4 surfaces (distinct ids enforced), a scroll
/// position, and focus on one of the surfaces or none.
fn arb_v0() -> impl Strategy<Value = SessionImage> {
    (1usize..=4)
        .prop_flat_map(|n| {
            let surfaces: Vec<_> = (0..n).map(|i| arb_surface(i as u32)).collect();
            (surfaces, 0.0f32..4096.0, any::<Option<prop::sample::Index>>())
        })
        .prop_map(|(mut surfaces, scroll, focus_idx)| {
            // Enforce distinct surface ids so coefficient lookup is unambiguous.
            for (i, s) in surfaces.iter_mut().enumerate() {
                s.id[0] = i as u8;
            }
            let focus = focus_idx.map(|idx| surfaces[idx.index(surfaces.len())].id);
            SessionImage::new("2026-06-29T00:00:00Z", surfaces, Layout { scroll_pos: scroll, focus })
        })
}

fn arb_bound() -> impl Strategy<Value = Bound> {
    prop_oneof![
        Just(Bound::NegInf),
        Just(Bound::PosInf),
        (-1.0e9f64..1.0e9).prop_map(Bound::Finite),
    ]
}

/// An arbitrary, possibly non-top coefficient. Timing is arity-1 so it is always
/// comparable to the (arity-1) semiring top.
fn arb_coefficient() -> impl Strategy<Value = Coefficient> {
    (
        arb_bound(),
        arb_bound(),
        prop_oneof![
            Just(Confidentiality::Secret),
            Just(Confidentiality::Restricted),
            Just(Confidentiality::Public),
        ],
        prop_oneof![Just(Integrity::Trusted), Just(Integrity::Endorsed), Just(Integrity::Untrusted)],
        prop_oneof![Just(Linearity::Omega), (0u64..1000).prop_map(Linearity::Finite)],
    )
        .prop_map(|(lo, hi, confidentiality, integrity, linearity)| Coefficient {
            timing: vec![Interval { lo, hi }],
            confidentiality,
            integrity,
            linearity,
        })
}

/// `I₁`: a v0 image plus a random coefficient per surface.
fn arb_v1() -> impl Strategy<Value = SessionImageV1> {
    arb_v0().prop_flat_map(|base| {
        let n = base.surfaces.len();
        proptest::collection::vec(arb_coefficient(), n).prop_map(move |coeffs| {
            let coefficients = base
                .surfaces
                .iter()
                .zip(coeffs)
                .map(|(s, coefficient)| SurfaceCoefficient { surface_ref: s.id, coefficient })
                .collect();
            SessionImageV1 { base: base.clone(), coefficients }
        })
    })
}

// ---- the laws ----

proptest! {
    /// L1 — `α(γ(img₀)) = img₀`, byte-identical after C14N (same content id).
    #[test]
    fn l1_roundtrip_is_identity(img0 in arb_v0()) {
        let back = alpha(&gamma(&img0));
        prop_assert_eq!(back.canonical_bytes(), img0.canonical_bytes());
        prop_assert_eq!(back.compute_id(), img0.compute_id());
    }

    /// identity-stability — the hash is invariant under abstraction, because it
    /// is computed over the base projection only.
    #[test]
    fn identity_is_base_determined(img0 in arb_v0()) {
        let img1 = gamma(&img0);
        prop_assert_eq!(img1.session_id(), img0.compute_id());
        prop_assert_eq!(alpha(&img1).compute_id(), img1.session_id());
    }

    /// L2 (operational) — `γ(α(img₁))` preserves the base and resets every
    /// coefficient to the semiring top. This is what the reference oracle checks.
    #[test]
    fn l2_resets_coefficients_to_top(img1 in arb_v1()) {
        let ga = gamma(&alpha(&img1));
        prop_assert!(ga.base_eq(&img1));
        prop_assert!(ga.coeff_is_top());
    }

    /// L2 (order) — under the spec's order (`⊤` greatest), the v1 round-trip is
    /// inflationary: `img₁ ⊑ γ(α(img₁))`, with equality iff `img₁` was all-top.
    #[test]
    fn l2_is_inflationary(img1 in arb_v1()) {
        let ga = gamma(&alpha(&img1));
        prop_assert!(img1.leq(&ga));
        if img1.coeff_is_top() {
            prop_assert!(ga.leq(&img1));
        }
    }

    /// The Galois connection: `α(x) ⊑ y  ⟺  x ⊑ γ(y)` for `x ∈ I₁`, `y ∈ I₀`.
    /// (Order on `I₀` is base equality, since both sides sit at coefficient `⊤`.)
    #[test]
    fn galois_connection(x in arb_v1(), y in arb_v0()) {
        let lhs = alpha(&x).canonical_bytes() == y.canonical_bytes();
        let rhs = x.leq(&gamma(&y));
        prop_assert_eq!(lhs, rhs);
    }

    /// The connection's non-trivial (true) branch: when `y = α(x)`, both sides
    /// hold — exercising the adjunction where bases agree.
    #[test]
    fn galois_connection_true_branch(x in arb_v1()) {
        let y = alpha(&x);
        prop_assert!(alpha(&x).canonical_bytes() == y.canonical_bytes());
        prop_assert!(x.leq(&gamma(&y)));
    }

    /// L3 — the renderer-migration law with an identity-stub v1 renderer
    /// (`checkpoint_v1 ∘ load_v1 = id`): `α(ckpt(load(γ(img₀)))) = img₀`.
    #[test]
    fn l3_migration_with_stub_renderer(img0 in arb_v0()) {
        let load_v1 = |s: SessionImageV1| s;
        let checkpoint_v1 = |s: SessionImageV1| s;
        let migrated = alpha(&checkpoint_v1(load_v1(gamma(&img0))));
        prop_assert_eq!(migrated.canonical_bytes(), img0.canonical_bytes());
        prop_assert_eq!(migrated.compute_id(), img0.compute_id());
    }

    /// The XML carrier is faithful: a v1 image survives serialise → parse.
    #[test]
    fn v1_xml_roundtrips(img1 in arb_v1()) {
        // to_xml writes the base-determined id; populate it so PartialEq aligns.
        let img1 = SessionImageV1 { base: img1.base.with_computed_id(), ..img1 };
        let parsed = SessionImageV1::from_xml(&img1.to_xml()).unwrap();
        prop_assert_eq!(parsed, img1);
    }

    /// Parser-level α: a v0 parser reading a v1 document recovers exactly the
    /// base (forward compatibility via the namespace split).
    #[test]
    fn v0_parser_drops_coefficients(img0 in arb_v0()) {
        let v1_xml = gamma(&img0).to_xml();
        let base = SessionImage::from_xml(&v1_xml).unwrap();
        prop_assert_eq!(base.canonical_bytes(), img0.canonical_bytes());
    }
}
