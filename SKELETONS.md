# SKELETONS

Intentional placeholders in the v0 scaffold: stubs, deferred subsystems, and
behaviour knowingly left out. This is a **live TODO ordered by impact, not a
changelog.**

Rules:

- **Record on leaving.** Whenever you leave a skeleton, add a matching entry
  here pointing at the code.
- **Delete on completion.** Remove the entry when the work lands. Absence means
  "done," not "forgotten."
- **Order by impact.** Blocking items first, polish last.
- **Be terse.** One or two lines; link to `docs/` for rationale rather than
  explaining here.

When the list grows past what's comfortable here, split it per crate
(`simulboot-<crate>/SKELETONS.md`).

---

## Blocking the demo

- **Platform capture backends** — `simulboot-host/src/{macos,windows,linux}`:
  `build_source` returns `NullCapture` (announce + input only, no frames). Real
  pipelines pending: macOS CGVirtualDisplay + ScreenCaptureKit + VideoToolbox;
  Windows WGC + NVENC/AMF (+ Linux VM via HCS API); Linux PipeWire + VA-API.
- **Real renderer** — `simulboot-client/src/render.rs`: `HeadlessRenderer` only
  logs strip state. The wgpu/Metal renderer + VideoToolbox decode plug into the
  same `Renderer` trait.

## Needed for interaction

- **Input loop** — `simulboot-client/src/conn.rs`: `Link::send_input` /
  `Link::host_addr` are unwired (`#[allow(dead_code)]`); no winit event loop
  feeds them yet. Host side routes input already.

## Robustness gaps

- **Datagram fragmentation** — `simulboot-host/src/main.rs` `send_frame`: frames
  larger than the path `max_datagram_size` are dropped with a warning. Fine while
  backends are stubs; needs fragmentation before real video.

## Tooling

- **cargo-deny license check not enforced** — `deny.toml` / `.github/workflows/ci.yml`:
  the gate runs `advisories bans sources` only. `deny.toml` carries a starter
  license allow-list, but it has not been validated against the real tree;
  validate it, then add `licenses` to the checked commands. (advisories — the
  bincode-class gate — *is* enforced.)

## Deferred to v1 (do not implement in v0)

- **Fast Infoset** — `simulboot-common/src/session.rs`: only text XML +
  canonical form exist; the binary Infoset form is out of scope.
- **Cap'n Proto wire format** — `simulboot-common/src/wire.rs`: postcard stands
  in for v0 (zero-copy parsing is the v1 motivation).
- **Multiparty session types** — `StructureMessage` is a plain typed enum
  standing in for the Scribble global type; the session-type checker layers on
  additively in v1.
