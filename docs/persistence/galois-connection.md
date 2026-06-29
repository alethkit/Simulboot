# Session Image Persistence: the v0 / v1 Galois Connection

## Purpose

This document specifies the abstraction (`α`) and concretisation (`γ`) maps
between v0 and v1 session images, and the laws they must satisfy so that a
session survives the migration from the v0 rendering pipeline (flat composite,
latest-frame-wins) to the v1 pipeline (retained scene graph, FIP, verified
timing) **without losing its identity**.

This is the formal content of the persistence telos: *the session is separable
from any particular renderer, including any particular renderer version.*

The maps are specified language-agnostically here, against the two XSD schemas
(`session-base-v1.xsd` and `session-coefficients-v1.xsd`). A Rust implementation
follows the spec; the spec is the artifact that outlives it.

---

## The two domains

Let:

- **`I₀`** = the set of valid **v0 session images**: documents valid against
  `session-base-v1.xsd` that contain *no* elements in the coefficients namespace.

- **`I₁`** = the set of valid **v1 session images**: documents valid against
  both schemas, containing exactly one `<coefficients>` block with one
  `<surface-coefficient>` per surface.

Both are ordered. The order is **information order on the coefficient dimension
only**; the v0-visible content (surface identities, host provenance, layout) is
required to be *equal*, not merely related, for two images to be comparable:

```
x ⊑ y   ⟺   base(x) = base(y)  ∧  coeff(x) ⊑_R coeff(y)
```

where `base(·)` is the v0-visible projection, `coeff(·)` is the per-surface
coefficient assignment, and `⊑_R` is the pointwise order of the coefficient
semiring `R = Interval^n × (Confidentiality × Integrity) × ℕ`.

In this order:
- `I₀` is the sub-poset where every surface's coefficient is *absent* — but we
  treat "absent" as identified with the semiring **top** `⊤` (see below). So
  `I₀ ≅ { x ∈ I₁ : coeff(x) = ⊤ on every surface }`.
- The semiring top `⊤` is: unbounded timing interval `[-∞,+∞]`, security label
  `(public, untrusted)`, linearity `omega`. This is *exactly* what v0 has
  implicitly — no timing guarantee, no IFC, free copying.

**Why top, not bottom.** v0 asserts *no restrictions*. In a grading where
smaller = more constrained, "no restriction" is the top. A v0 image is the
maximally-permissive coefficient assignment. This is the crux: it is what makes
`γ` (which must produce a v0-compatible image when fed a v0 image) land exactly
on `I₀`.

---

## The maps

### α : I₁ → I₀  (abstraction — "forget the coefficients")

```
α(doc) = delete every element in namespace
         "https://simulboot.dev/session/v1/coefficients" from doc
```

That is the entire definition. Because the coefficients live in a separate
namespace as a *sibling* of `<surfaces>` and `<layout>` (not nested inside
`<surface>`), `α` is a single subtree deletion. No surgery inside surface
elements. The result is a document in `I₀`.

`α` is monotone: if `x ⊑ y` then `base(x) = base(y)`, so `α(x) = α(y)`; the
coefficient order is collapsed to a point, trivially preserving `⊑`.

### γ : I₀ → I₁  (concretisation — "embed at top")

```
γ(doc) = doc  with a <coefficients> block inserted, containing one
         <surface-coefficient surface-ref="{id}"> per <surface id="{id}">,
         each set to the semiring top:
           <timing><delay-path lo="-INF" hi="+INF"/></timing>
           <security confidentiality="public" integrity="untrusted"/>
           <linearity count="omega"/>
```

`γ` is monotone: it is constant in the coefficient dimension (always `⊤`), so it
trivially preserves any order on its input.

---

## The Galois connection

**Claim.** `(α, γ)` form a **Galois connection** between `I₁` and `I₀`, with `α`
the lower adjoint and `γ` the upper adjoint:

```
α(x) ⊑ y   ⟺   x ⊑ γ(y)            for all x ∈ I₁, y ∈ I₀
```

**Proof sketch.**
- (`⇒`) Suppose `α(x) ⊑ y`. Since `y ∈ I₀`, `coeff(y) = ⊤`. `α(x) ⊑ y` requires
  `base(x) = base(α(x)) = base(y)`. Then `γ(y)` has `base(γ(y)) = base(y) =
  base(x)` and `coeff(γ(y)) = ⊤ ⊒ coeff(x)`. Hence `x ⊑ γ(y)`.
- (`⇐`) Suppose `x ⊑ γ(y)`. Then `base(x) = base(γ(y)) = base(y)`, so
  `base(α(x)) = base(x) = base(y)`, and `coeff(α(x))` collapses to a point
  `⊑ coeff(y) = ⊤`. Hence `α(x) ⊑ y`. ∎

The connection is moreover a **complete abstraction** (a.k.a. a reflection /
insertion): `α ∘ γ = id` on `I₀` (see law L1 below). This is the strong form —
the round trip through v1 loses *nothing* of a v0 image. It is what makes
forward migration lossless.

---

## The persistence laws

### L1 — v0 round-trip is the identity (forward compatibility, lossless)

```
α(γ(img₀)) = img₀        for all img₀ ∈ I₀
```

A v0 session image, concretised into v1 and abstracted back, is **byte-identical**
after C14N — therefore has the **same content-addressed session id**.

*Consequence:* a session checkpointed by a v0 compositor can be resumed by a v1
compositor (`γ`) and re-checkpointed to a form an old v0 compositor reads back
identically (`α`). The session keeps its identity across the renderer upgrade.

This is the law that licenses "colonising the rendering pipeline": v1 may rewrite
the entire renderer, and every pre-existing v0 session still resumes and
re-persists with a stable hash.

### L2 — v1 round-trip is inflationary (backward compatibility, lossy-by-exactly-the-coefficients)

```
img₁ ⊑ γ(α(img₁))        for all img₁ ∈ I₁
```

with equality **iff** `coeff(img₁) = ⊤` on every surface (i.e. iff `img₁ ∈ I₀`
already).

This is the standard Galois orientation: with `α` the lower adjoint and `γ` the
upper, the unit `id ⊑ γ∘α` is inflationary. Concretely, `γ` embeds at the
semiring **top** `⊤`, and `⊤` is the *greatest* element of the order (most
permissive — see "Why top, not bottom" above), so `γ(α(img₁))` sits *above*
`img₁`: it is `img₁` with every coefficient raised back to `⊤`.

The residual `γ(α(img₁)) ⊖ img₁` is *exactly* the coefficient information: the
timing intervals, security labels, and linearity counts that a v0 renderer
cannot represent. The `⊑` (not `=`) is the honest statement of what degrades
when an old renderer touches a new session: the session content survives, its
graded refinements are reset to top.

*Consequence:* a v1 session can be down-migrated for a v0 compositor (`α`), but
round-tripping it through v0 forgets timing/security/linearity — every
coefficient comes back at `⊤`. This is a controlled, fully-characterised loss —
not corruption. The lattice names exactly what is lost.

> **Orientation note.** An earlier draft wrote this law as `γ(α(img₁)) ⊑ img₁`
> and called it "deflationary." Under this document's own order convention — `⊤`
> is the greatest element, `α` is the lower adjoint, `γ` the upper — that
> direction is reversed: `γ∘α` raises coefficients to `⊤` and is therefore
> inflationary, as written above. The Rust implementation
> (`simulboot-common/src/galois.rs`) and its property tests follow the
> consistent orientation, and additionally check the operational form below
> (base preserved ∧ every coefficient reset to `⊤`), which the reference oracle
> verifies and which holds regardless of how the order is named.

### L3 — the renderer migration law (the actual telos)

This is the law the whole exercise exists to establish. Let:

- `load_v0 : I₀ → RenderState₀`, `checkpoint_v0 : RenderState₀ → I₀`
- `load_v1 : I₁ → RenderState₁`, `checkpoint_v1 : RenderState₁ → I₁`

be the reconstitution and checkpoint maps of each renderer. Require, of each
renderer independently, that checkpoint inverts load on the persisted content:

```
checkpoint_v0 ∘ load_v0 = id   on I₀
checkpoint_v1 ∘ load_v1 = id   on I₁
```

(Each renderer faithfully persists what it loaded — surfaces, layout, focus,
scroll, and for v1 also coefficients. This is a per-renderer obligation, checked
against that renderer.)

Then the **migration law** is:

```
α( checkpoint_v1( load_v1( γ(img₀) ) ) ) = img₀     for all img₀ ∈ I₀
```

In words: take any v0 session → concretise to v1 → load into the v1 renderer →
run → checkpoint → abstract back to v0. You recover the original v0 image
exactly (same C14N hash).

**This is provable from L1 + the two per-renderer obligations**, with no
reference to *how* either renderer works:

```
  α(checkpoint_v1(load_v1(γ(img₀))))
=   { v1 obligation: checkpoint_v1 ∘ load_v1 = id }
  α(γ(img₀))
=   { L1 }
  img₀
```

So the renderers are quotiented out entirely. The migration is sound **iff** L1
holds and each renderer round-trips its own persisted content. The rendering
pipeline can be rewritten arbitrarily — flat composite to retained scene graph
to anything — and every session survives, *provided each renderer satisfies its
own checkpoint∘load = id obligation and the α/γ pair satisfies L1.*

---

## Relationship to the rest of the verification story

Three distinct structures, kept separate (do not collapse them):

1. **Each renderer ↔ the session denotation** `SurfaceId → GRV r Frame`:
   certified via Elliott's commuting square (Timely Computation 2023). This is
   *rendering correctness* — that what's displayed respects the discrete session
   meaning. v0 and v1 are *parallel* legs over a shared denotation, not two ends
   of an adjunction.

2. **v0 ↔ v1 session images**: the Galois connection `(α, γ)` specified here.
   This is *persistence survival* — that sessions keep their identity across
   renderer versions. Laws L1–L3.

3. **exact continuous timing ↔ interval timing** (internal to v1): a separate
   Galois connection in the timing-interval domain, which Elliott proves is
   *exact* (linear, no precision loss). This is v1-internal timing verification
   and does not involve v0.

The coefficient namespace split in the XSD is the syntactic carrier of structure
(2): `α` = "project out the coefficient namespace", a pure XML operation. That
the split is a *namespace* (not just a convention) is what makes `α`
schema-checkable and makes a v0 parser accept v1 documents via the `xs:any`
lax-processing wildcard in the base schema (forward compatibility at the parser
level, independent of L1's content-identity level).

---

## What to implement, and what to test

### Implement (in `simulboot-common`, after this spec)

- `alpha(doc: &Xml) -> Xml` — delete the coefficients-namespace subtree.
- `gamma(doc: &Xml) -> Xml` — insert a top-valued `<coefficients>` block, one
  `<surface-coefficient>` per `<surface>`.
- `c14n(doc: &Xml) -> Vec<u8>` — the canonicalisation used for the session id.
- `session_id(doc: &Xml) -> [u8; 32]` — `SHA256(c14n(base(doc)))`, i.e. the hash
  is computed over the **base projection only**, so that a v1 image and its α
  share a session id. (This is a design choice: identity is base-determined.
  It is what makes L1 hold at the level of *identity*, not just structure.)

### Test (property tests, runnable in v0 before v1 exists)

- **L1**: `∀ img₀ ∈ I₀. alpha(gamma(img₀)) == img₀` (after C14N).
- **L2-operational** (the form the oracle checks, order-name-independent):
  `∀ img₁ ∈ I₁. base(gamma(alpha(img₁))) == base(img₁)`
  and `coeff(gamma(alpha(img₁))) == ⊤`.
- **L2-order** (inflationary): `∀ img₁ ∈ I₁. img₁ ⊑ gamma(alpha(img₁))`, with
  equality iff `img₁` is already all-`⊤`.
- **L2-equality-case**: `∀ img₀ ∈ I₀. gamma(alpha(gamma(img₀))) == gamma(img₀)`.
- **identity-stability**: `∀ img₁ ∈ I₁. session_id(img₁) == session_id(alpha(img₁))`
  — the hash is invariant under abstraction (because it is computed over base).
- **L3** becomes testable once a v0 renderer exists: feed `gamma(img₀)` through a
  *stub* `load_v1`/`checkpoint_v1` that preserves coefficients, and check the
  composite returns `img₀`. The stub stands in for the real v1 renderer and
  encodes the per-renderer obligation as a test fixture.

The L1 and identity-stability tests are the ones to write **first**, against the
v0 session image type, before any v1 work begins. They establish the contract
that the v1 renderer must later honour, and they are pure XML/hash properties —
no rendering, no GPU, no network.

---

## Generator for property tests (shape)

A `proptest` strategy for `I₀`:

- 1–4 surfaces, each with:
  - a random 32-byte id rendered as `sha256:{hex}` (or a content hash of the
    surface's other fields, for realism)
  - name from {macOS, Windows, Linux, ...}
  - order = index
  - host: random tailnet-shaped address, os ∈ {macOS, Windows, Linux},
    machine name, capture string
  - codec ∈ {H265, AV1}
  - dimensions from a small set of plausible resolutions
- layout: scroll-pos ∈ [0, total_width], focus = one of the surface ids.

`I₁` generator = `I₀` generator composed with a coefficient-assigner that picks,
per surface, a random (possibly non-top) coefficient. Used for L2 tests.
