# The Simulboot Substrate: Denotation

This document states what the Simulboot substrate **means**. It is the
theoretical spine of the project: a denotational specification from which the
demo (v0) and the eventual substrate (v1) both follow as implementations.

The central claim is one sentence:

> **v0 and v1 are two implementations of a single denotation, differing only in
> how precise their coefficient is.**

Everything below builds toward that claim and then unpacks what it buys us. The
document is self-contained — every load-bearing term is defined where it first
appears, and the cited papers are *further reading*, not prerequisites. A reader
comfortable with functional programming and basic algebra should be able to
follow it cold.

The two companion documents are `SIMULBOOT_HANDOFF.md` (the v0 build brief and
the prose thesis) and `galois-connection.md` (how a saved session survives a
renderer upgrade). This document is the layer underneath both: it says what a
session *is*, so that the other two can say how it is built and how it persists.

---

## 0. Method: two borrowed commitments

The whole document rests on two ideas, both from Conal Elliott. They are stated
here in full so nothing later depends on outside reading.

### Denotational design

*(Elliott, "Denotational Design with Type Class Morphisms," LambdaPix TR
2009-01.)*

A type is specified by giving its **meaning**: a function `⟦·⟧` ("semantic
brackets") from values of the type to a mathematical model chosen for precision
and simplicity. The implementation is then *anything* that realises that meaning
efficiently — the model is the spec, the code is a detail.

An operation `op` is **correct** exactly when its meaning is a homomorphism:

```
⟦ op(args) ⟧  =  op( ⟦args⟧ )
```

read as "the meaning of the implementation equals the corresponding operation on
the meaning." Two values are equal exactly when their meanings are equal:

```
a ≡ b   ⟺   ⟦a⟧ ≡ ⟦b⟧
```

Semantic equality is the *only* equality that matters. This is the discipline we
hold every part of the substrate to.

### Timely computation

*(Elliott, "Timely Computation," Proc. ACM Program. Lang. 7 ICFP art. 219,
2023.)*

The slogan: **"a digital circuit is an analog circuit that respects discrete
meanings."** A physical implementation runs in continuous time and is subject to
delays; a correct one nonetheless realises a clean discrete specification. The
formal content is a commuting square (Elliott's Fig. 1): for an implementation
`f̃` (continuous) of a specification `f` (discrete),

```
k (Φ_m f̃ ũ)  ≡  φ_m f (h ũ)
```

where `h` extracts the discrete meaning of the inputs, `k` extracts the discrete
meaning of the output, and `Φ_m`/`φ_m` are the continuous and discrete
"running" of the system. Read aloud: *running the real circuit and then
extracting its meaning gives the same answer as extracting the meaning of the
inputs and running the spec.*

The key ingredient is **stability**: a value is stable over a time *interval*
when its discrete interpretation is constant across that interval. Timing
analysis phrased in terms of intervals (not instants) turns out to be **linear
over a semiring** — composing two stages adds their delay intervals, running two
stages in parallel takes the max. We use this result twice: as the correctness
criterion for the compositor (§5), and as the timing dimension of the
coefficient (§2).

---

## 1. Background you need

A compact vocabulary. Each term is used heavily later; skim now, refer back as
needed.

- **Signal / Behavior** — a value that varies over continuous time, denoted
  `Signal a = Time → a`. This is Elliott's `Behavior a` from functional reactive
  programming (FRP). The semantics needs only that `Time` is totally ordered;
  think `Time = ℝ`.

- **Event** — discrete timed occurrences, `Event a = [(Time, a)]`: a list of
  (time, value) pairs. Signals are continuous; events are pointwise.

- **Semiring** — a set with two operations (`+`, `×`) and their units (`0`,
  `1`), where `+` is commutative-monoidal and `×` distributes over it. The point
  for us: semirings compose *linearly*, and a direct product of semirings is
  again a semiring (so independent dimensions can be bundled into one).

- **Comonad** — the dual of a monad. Where a monad wraps a value you can put
  *into* a context, a comonad `D` is a context you can read a value *out of*:
  `extract : D a → a` (read the focused value) and `duplicate : D a → D (D a)`
  (expose the surrounding context). Comonads model *context-dependent* values.

- **Graded comonad** — a comonad indexed by an element of a semiring:
  `D_r a` carries a grade `r ∈ R`. The grade tracks *how much context* the value
  depends on, and the comonad laws relate grade composition to the semiring's
  `×` and `1`. This is the technical home of a "coefficient."

- **Coeffect** — the demand a computation places on its context (how it is used:
  its timing budget, its security clearance, how many times it may be consumed),
  as opposed to an *effect* (what a computation does to the world). Coeffects are
  exactly what graded comonads track. *(Petricek, Orchard, Mycroft, "Coeffects: a
  calculus of context-dependent computation," 2014.)*

- **IFC label lattice** — an information-flow-control lattice ordering data by
  sensitivity, e.g. confidentiality (`public ⊑ … ⊑ secret`) and integrity
  (`trusted ⊑ … ⊑ untrusted`). *(Denning; Goguen–Meseguer lineage.)*

- **Content addressing** — naming a value by the hash of its contents, so the
  name is machine-independent and tamper-evident. A `SurfaceId` is such a hash.

- **Push–pull FRP** — the standard efficient implementation of FRP as a directed
  acyclic graph (DAG) of nodes: inputs *push* changes in (marking dependents
  dirty), and consumers *pull* results out on demand (recomputing only dirty
  nodes). *(Realised in OCaml by Bünzli's React and its successor Note.)*

- **FIP — functional in-place update** — when a value is uniquely owned (no other
  reader holds it), an update may overwrite it in place instead of allocating a
  fresh copy, with no observable difference. Uniqueness is the enabling fact, and
  it is exactly a linearity count of 1.

- **vsync** — the display's vertical-sync instant: the moment the compositor must
  hand a finished frame to the screen. It is the demand signal for the whole
  scene.

With this vocabulary the rest of the document is self-contained.

---

## 2. The core denotation

Everything in the substrate is a value of one type:

```
GRV r a  =  D_r (Time → a)
```

a **graded reactive value**: a value of type `a` varying over continuous `Time`
(`Time → a`, an FRP behaviour), wrapped in the grade-carrying context `D_r`. The
grade `r`, drawn from the semiring `R` of §3, is the coefficient.

`GRV r a` is just a `Signal a` *with a coefficient attached*. That coefficient is
the entire difference between this and ordinary FRP: it carries timing, security,
and linearity in the type itself. How those three live in one grade is §3; why a
single grade suffices for all three is §7.

---

## 3. The grading semiring

The coefficient is drawn from a product of three independent semirings:

```
R  =  Interval^n  ×  (Confidentiality × Integrity)  ×  ℕ
```

Because a direct product of semirings is a semiring, these three dimensions
combine cleanly into one grade with no interaction between them. (Elliott uses
exactly this construction in Timely Computation to fuse the rising- and
falling-edge interval semirings `I↑`, `I↓` into one interval semiring `I`.)

| Dimension | Carrier | Composition | Top element `⊤` (most permissive) |
|---|---|---|---|
| **Timing** | `Interval^n` — a multi-dimensional time interval, one dimension per input-to-output delay path (Elliott's stability interval) | sequential composition **adds** intervals (latency accumulates); parallel composition takes the **max** (the slower path governs) | `⊤_time = [-∞, +∞]` — "no timing guarantee" |
| **Security** | `Confidentiality × Integrity` — an IFC label lattice | lattice join | `⊤_sec = (public, untrusted)` — "no restriction asserted" |
| **Linearity** | `ℕ` extended with `ω` — a usage count | add counts | `⊤_lin = ω` — "may be consumed any number of times" |

The semiring top is the triple of tops:

```
⊤  =  (⊤_time, ⊤_sec, ⊤_lin)  =  ([-∞,+∞], (public, untrusted), ω)
```

`⊤` is the **maximally permissive** coefficient: no timing guarantee, no security
restriction, unrestricted copying. This is precisely the coefficient an
*unannotated* system carries — which is the hinge of the whole refinement story
(§6).

> A note on orientation, since it trips people up: throughout, **smaller means
> more constrained** and `⊤` is the *greatest* element. A tight timing bound, a
> secret label, a linearity of 1 are all *below* `⊤`. The companion
> `galois-connection.md` depends on this same orientation.

---

## 4. The system as one morphism

The entire compositor is a single morphism in the graded category:

```
system : D_r World → GRV r DisplayFrame
```

`World` is every input — hardware events, host surface streams, network events.
The system is a time-varying display computed from a time-varying world, carrying
a coefficient `r` that simultaneously bounds its timing, secures its information
flow, and tracks its linearity.

### The session is a content-addressed collection of reactive surfaces

The object the user actually inhabits is the **session**. Its denotation is
forced by simple rewriting. Start from a time-varying assignment of frames to
surfaces and push the time inside (this is Elliott's `TMap` reasoning — a *total*
map, discussed below):

```
Session = Time → (SurfaceId → Frame)     -- a time-varying total map of surfaces
        = SurfaceId → (Time → Frame)      -- flip: a map of time-varying frames
        = SurfaceId → Surface             -- since Surface = Time → Frame
        = SurfaceId → GRV r Frame         -- graded: each surface carries a coefficient
```

The last line is the payoff:

> **A session is a content-addressed collection of graded reactive surfaces.**

`SurfaceId` is a content hash — the key; the surface is a graded signal — the
value. This is why content-addressing and reactivity are *two projections of one
structure*: they are simply the outer (`SurfaceId → …`) and inner (`… → (Time →
Frame)`) readings of the same map `SurfaceId → (Time → Frame)`.

### Clean disconnect falls out of the denotation

The map is **total** (a `TMap`, in Elliott's terminology — partiality factored
*out* of an ordinary `Map`, so a missing key yields a default rather than being
absent). A disconnected surface therefore does not vanish from the session; it
holds its last value. That held value is exactly a signal read in its own past:

```
analog1 δ h x̃  =  λt. h (x̃ (t − δ))
```

a signal delayed by `δ` is the original signal evaluated `δ` ago. So
"surface froze on disconnect" is not a special case in the code — it is what the
denotation already says happens. This is the first of several behaviours we get
*for free* by fixing the meaning first.

---

## 5. The four projections

The unification claim of the substrate is that reactivity, view, persistence, and
containment are **not four subsystems bolted together**. They are four
*forgetful functors* out of the one graded reactive category — each forgets a
different part of `GRV r a`:

```
F_react   : GRV r a       → (Time → a)        -- forget the coefficient, keep continuous time
F_view    : GRV r Visual  → DisplayFrame       -- sample the signal at vsync
F_persist : GRV r a       → (SequenceNo → a)   -- evaluate over discrete pseudo-time
F_contain : GRV r a       → r                   -- extract the coefficient, forget the value
```

- **`F_react` — the reactive reading.** Forget the coefficient and keep the
  continuous behaviour. Implemented as a push–pull reactive DAG (Bünzli's
  React/Note style): WASM clients and hardware drivers *push* events to source
  nodes, marking dependents dirty; the compositor *pulls* at vsync via a
  rank-ordered depth-first traversal, recomputing only dirty nodes. Rank order
  excludes diamond reglitches. Where a node is uniquely owned (linearity 1 in its
  coefficient), FIP updates it in place, so the hot path allocates nothing.

- **`F_view` — the display reading.** Sample the visual signal at the vsync
  instant. This is the *demand* side of push–pull: vsync is the demand for the
  whole scene graph. Its correctness is precisely the commuting square of §6.

- **`F_persist` — the persistence reading.** Evaluate the signal over discrete
  *pseudo-time*, where each committed state carries a `SequenceNo` (Reed's notion
  of logical sequence). This is orthogonal persistence: undo, history, sync,
  crash recovery, and publication are *the same mechanism* because they are one
  projection. The content-addressed root hash unifies with the sequence number in
  the commit log — identity and history are the inner and outer of the same map
  again.

- **`F_contain` — the containment reading.** Extract the coefficient and ignore
  the value. Capability checking, IFC enforcement, and timing verification are
  all "read `r`, then check it against a lattice." A capability gate is exactly
  `F_contain` followed by a lattice comparison.

The interaction laws (stated here, to be mechanised — see §9):

1. reactivity and persistence commute up to pseudo-time;
2. `F_view` is `F_contain`-respecting — it never displays what the coefficient
   forbids;
3. all four projections agree on the underlying value wherever their domains
   overlap.

---

## 6. Correctness is the commuting square

The compositor is correct exactly when Elliott's timely-computation square (§0)
commutes, instantiated for surfaces:

| Elliott's term | Simulboot instantiation |
|---|---|
| `f̃` — continuous implementation | the GPU pipeline producing frames over physical time |
| `f` — discrete specification | the session `SurfaceId → Frame` at each logical vsync |
| `h` — meaning of the inputs | sampling host surface streams at vsync |
| `k` — meaning of the output | what the user sees at each refresh |
| `⟳` — the commutativity proof | the displayed frame respects the discrete session despite delays |

The condition `k (Φ_m f̃ ũ) ≡ φ_m f (h ũ)` reads: *the displayed frame is the
correct composite of what the hosts sent, accounting for their delays.* Two
slogans follow directly, and they are the same theorem, not an analogy:

- **A tear is a timing glitch** — an output sampled while an input is unstable.
- **Vsync is the stability discipline** — the requirement that each surface's
  frame be held constant across the sampling instant.

The substrate avoids tearing by the identical mechanism a digital circuit avoids
glitches: require the inputs stable across the sampling interval.

The delay term is literal. The frame composited at vsync time `t` for a surface
with network delay `δ` is the host's frame from `t − δ` — exactly `analog1 δ`
from §4. The difference between the two versions is *only how `δ` is treated*:

- **v0:** `δ` is implicit — latest-frame-wins, whatever the QUIC round-trip
  happens to be.
- **v1:** `δ` is the explicit timing coefficient, and the composite's timing is
  verified by linear algebra over the interval semiring — Elliott's verified
  timing analysis, transported from logic gates to surfaces.

---

## 7. The refinement: v0 is `GRV ⊤ a`

This is the single most important structural fact for anyone implementing the
system:

> **v0 implements the same denotation as v1, at coefficient `r = ⊤`.**

v0's session is `Session = Time → (SurfaceId → Frame) = SurfaceId → GRV ⊤ Frame`.
The coefficient is top because v0 leaves all three dimensions implicit:

| Dimension | v0 (implicit, `= ⊤`) | v1 (explicit, tightened below `⊤`) |
|---|---|---|
| Timing | whatever QUIC RTT happens to be | typed `Interval^n`, verified linearly |
| Security | Tailscale transport only | per-surface IFC label, capability-gated |
| Linearity | frames copied freely | uniqueness tracked, FIP in place |

So v0 → v1 is **not a rewrite.** It is *tightening `r` toward the correct
coefficient for each morphism by making explicit what v0 left implicit.* The
denotation is fixed; the coefficient becomes precise.

The mechanism that takes you from `GRV ⊤ a` down to `GRV r a` (for `r ⊑ ⊤`) is
the **grading action of the comonad** — the structural map induced by the order
relation `r ⊑ ⊤`, which weakens a value to a more permissive grade. It is *not* a
Galois connection between renderers. Keep three structures distinct:

1. **The grading action** (this section) — relates `GRV ⊤ a` and `GRV r a`
   *inside one renderer's denotation*.
2. **The persistence Galois connection** (`galois-connection.md`) — relates v0
   and v1 session *images*, one level up, so saved sessions survive a renderer
   change.
3. **An exact timing Galois connection** (inside v1) — between exact continuous
   timing and interval timing; Elliott proves it loses no precision.

Conflating these is the most common way to misread the architecture.

### What survives v0 → v1 unchanged — build these correctly in v0

- **`SurfaceId` is a content hash**, the seed of the persistence projection.
  Never a human-readable name.
- **The wire protocol is a typed message algebra** — the degenerate case of a
  session type. The v1 multiparty session type (a Scribble global type) layers on
  *additively*, so keep v0's messages typed.
- **The strip / `Surface` types are the degenerate scene graph** — the leaf case
  of v1's morphism-annotated scene tree.
- **The XML-Infoset session image with C14N identity is the degenerate
  persistence domain** — a single checkpoint. v1 adds a `<parent>` link and it
  becomes a commit log.

### What is genuinely discontinuous — v0's is throwaway

**The rendering pipeline**, and only it. v0 is a simple wgpu/Metal composite per
vsync; v1 is a retained scene graph with FIP in-place update and Elliott timing
verification. These are different architectures: the v0 renderer does not refine
into the v1 renderer. Everything else refines cleanly.

This is acceptable and expected — and it is *exactly why the persistence Galois
connection matters.* Because the renderer is replaced rather than refined, the
guarantee that sessions survive the replacement cannot come from the renderer; it
comes from the session-image laws (`galois-connection.md`, law L3).

---

## 8. Why one semiring suffices: the coeffect bridge

Why can timing, security, and linearity share a single coefficient? Because they
are all **graded comonads over different semirings**, and coeffects unify graded
comonads (Petricek–Orchard–Mycroft, 2014). The "lattice people" (IFC: Denning,
Goguen–Meseguer) and the "capability people" (object-capabilities: Miller;
linear types) are doing the same thing over different semirings. The
product-semiring construction (§3) smashes them into one grade.

Two flavours of coeffect matter here:

- **Flat coeffects** — grade the *whole* context at once. These are exactly
  OxCaml's *modes* (`@local`, `@unique`, `@once`, `@uncontended`). The substrate's
  hard-path obligations — `[@zero_alloc]`, and uniqueness for FIP — are flat
  coeffects the OxCaml compiler checks for free.
- **Structural coeffects** — grade *per variable*. Needed for fine-grained
  per-input security and timing analysis. Deferred past v0; the flat fragment
  suffices for the demo.

Note what is deliberately *absent*: **effects are not the model for the
programming layer.** Elliott's objection to monadic `IO` ("a way *not* to bring
IO into the functional world") applies — the substrate's programming model is
coeffects-only. Session types appear **only** at the WASM/network boundary, where
a stateful sync-vs-delta protocol genuinely needs sequencing that `Signal` and
`Event` alone do not express.

---

## 9. What the demo proves, denotationally

Simulboot is a counter-rebuttal to Stephen Kell's argument that an operating
system is necessary (`SIMULBOOT_HANDOFF.md` carries the prose thesis). The
denotational form of the argument:

Kell traces the file abstraction back to a Smalltalk-style object and concludes
that an OS is necessary to host such objects. The counter: the true limit of the
file abstraction is **not an object** but a **content-addressed reactive
signal** —

```
SurfaceId → GRV r Frame
```

— whose identity (a hash) is machine-independent and whose update is typed with a
coefficient. The composition unit that spans machines is therefore the
**session**, not the file. And because the session is denotationally
`SurfaceId → GRV r Frame`, it is **portable by construction**: every component of
it — hash identity, signal value, coefficient — is independent of any OS
instance.

The demo exhibits a value of this type spanning three OS instances and surviving
migration. That is an existence proof: the denotation is realisable; hence the
session/OS separation is achievable; hence Kell's "an OS is still necessary"
holds only for *infrastructure per machine*, not for the thing the user inhabits.

---

## 10. Open denotational questions

Directions for the verification effort, in rough dependency order:

1. **Mechanise the four projections (§5) as forgetful functors** in Lean — show
   each is functorial and that the three interaction laws hold. Elliott's
   comma-category construction in Agda (`--safe --without-K`, no postulates) is a
   direct template, with `Frame` for the value type and the session map for
   multi-signals.
2. **Prove the timing dimension is a semiring** and that morphism composition is
   linear over it — Elliott's result, re-proved for the surface morphism algebra
   rather than logic gates.
3. **Prove the product grading is a graded comonad** — that the smash of the
   three dimensions (§3) satisfies the graded-comonad laws.
4. **Connect to denotational SSA** (Ghalayini & Krishnaswami, arXiv:2411.09347):
   the morphism algebra compiles to SSA; the denotation of the compiled form
   should agree with `⟦·⟧` here — a second commuting square, at the compilation
   boundary.

---

## Further reading

The citations above are supportive, not prerequisite. In order of centrality:

- Conal Elliott, *Denotational Design with Type Class Morphisms*, LambdaPix
  TR 2009-01 — the method of §0.
- Conal Elliott, *Timely Computation*, Proc. ACM Program. Lang. 7 ICFP
  art. 219 (2023) — the correctness square of §6 and the timing semiring of §3.
- Petricek, Orchard, Mycroft, *Coeffects: a calculus of context-dependent
  computation* (2014) — the bridge of §8.
- Denning; Goguen–Meseguer — IFC label lattices (§3 security dimension).
- Daniel Bünzli, *React* / *Note* — the push–pull FRP realisation of §5.
- Ghalayini & Krishnaswami, *Denotational SSA*, arXiv:2411.09347 — §10, item 4.

## Companion documents

- `SIMULBOOT_HANDOFF.md` — the v0 build brief and the prose Kell rebuttal.
- `galois-connection.md` — session-image persistence across renderer versions
  (the second of the three structures distinguished in §7).
