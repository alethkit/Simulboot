# 0000 — Roadmap

- **Status:** Accepted
- **Skeletons:** all current entries in `SKELETONS.md`
- **Updated:** 2026-06-29
- **Summary:** the arc from the present headless v0 spine to a visible
  three-OS demo, and from there to the v1 coefficient tightening.

This is the rolling overview. Each phase links to the focused plans that carry
it out. The denotation is fixed throughout (`docs/denotation.md`); we are
*tightening the coefficient*, not changing the meaning.

## Where we are

The cross-platform spine is implemented and loopback-verified: QUIC transport,
the wire protocol, the session image with content addressing, the compositor
strip, suspend → checkpoint → serve → resume → reconnect, and the v0⇄v1
persistence Galois connection. Everything that touches a real screen, GPU, or
input device is a documented stub (`SKELETONS.md`).

## Phase A — one visible surface

Make a single real surface appear and respond, end to end on the demo machine.

- One platform `CaptureSource` backend producing real encoded frames
  ([`0001-capture-backends.md`](0001-capture-backends.md), first OS only).
- The wgpu/Metal renderer behind the existing `Renderer` trait, decoding and
  presenting those frames.
- The winit event loop wiring `Link::send_input` so input reaches the host.

Exit: capture a window on the demo machine, see it composited, type into it.

## Phase B — the three-OS strip

Breadth across the surfaces the thesis needs.

- The remaining capture backends (the other two OSes), all behind the same
  trait — the compositor stays per-OS-code-free (Claim B).
- Multiple surfaces live in the strip simultaneously, scroll/focus working
  against real content.

Exit: macOS + Windows + Linux-VM surfaces in one strip on one compositor.

## Phase C — migration hardening

Make suspend/resume robust with real video flowing.

- Datagram fragmentation so frames above the path MTU survive
  (`SKELETONS.md`, robustness).
- Suspend → resume on a *second physical device* with live surfaces, verifying
  the content-addressed image round-trips and surfaces reconstitute.

Exit: the headline demo — suspend on device 1, resume on device 2, hosts keep
running (Claim C).

## Phase D — v1 tightening (post-demo)

Make explicit what v0 leaves implicit; `r = ⊤` becomes a real coefficient
(`docs/denotation.md §6–7`). Not a rewrite of anything except the renderer.

- Timing: typed `Interval^n`, verified linearly over the interval semiring.
- Security: per-surface IFC label, capability-gated.
- Linearity: uniqueness tracking and FIP in-place update in the retained scene
  graph.
- Persistence: add a `<parent>` link to the session image, turning the single
  checkpoint into a commit log; the persistence Galois connection
  (`docs/persistence/galois-connection.md`) guarantees v0 sessions survive the
  renderer replacement (law L3).

Each of these gets its own plan when Phase D opens.

## Invariants that gate every phase

From `docs/denotation.md §7` and `CLAUDE.md` — do not regress these while
building:

- compositor has no per-OS code (Claim B);
- host persists across suspend (Claim C);
- `SurfaceId` is a content hash; session id is base-determined;
- wire messages stay typed.
