# Simulboot

> The OS is necessary on each machine as infrastructure, but the thing the user
> inhabits — the *session* — should be separable from any specific OS instance,
> portable across machines, and self-describing.

Simulboot is a proof-by-construction counter-argument to the conclusion of
Stephen Kell's *"The Operating System: Should There Be One?"* (PLOS 2013). It
applies the Squeak/Pharo image model correctly: not absorbing the OS into the
language, but **separating the session from the OS**. OS instances are ports.
The session is the ship.

The v0 demo runs a unified compositor on a MacBook that simultaneously displays
surfaces captured from macOS, Windows, and a Linux VM; the session can be
**suspended** to a content-addressed image and **resumed** on a second device,
where all surfaces reconstitute automatically while the source OS instances keep
running.

See [`SIMULBOOT_HANDOFF.md`](SIMULBOOT_HANDOFF.md) for the full v0 brief — this
repository implements that scope and nothing beyond it —
[`docs/denotation.md`](docs/denotation.md) for the theoretical spine (what a
session *means*: one denotation, with v0 and v1 as the same thing at different
coefficients), and [`docs/persistence/`](docs/persistence/) for how a saved
session survives a renderer upgrade.

## Workspace layout

```
simulboot/
├── simulboot-common/   Shared types: wire protocol + session-image format
├── simulboot-host/     Runs on each source machine; captures one surface
├── simulboot-client/   The compositor: strip layout, session suspend/resume
└── simulboot-broker/   Minimal HTTP server that parks a suspended session image
```

## Status

This is the initial scaffold. The cross-platform spine is implemented, tested,
and runs end-to-end (QUIC loopback → announce → strip → suspend → checkpoint →
serve → resume → reconnect). Platform-native capture/encode/render backends are
stubbed with documented integration points, gated by target so the default
build is cross-platform.

| Area | State |
| --- | --- |
| `simulboot-common` wire protocol (bincode, length-prefixed) | ✅ implemented + tested |
| `simulboot-common` session image (XML, C14N content addressing) | ✅ implemented + tested |
| `simulboot-common` v0⇄v1 persistence Galois connection (α/γ, laws L1–L3) | ✅ implemented + property-tested |
| `simulboot-broker` HTTP session-image server | ✅ implemented + tested |
| Compositor strip layout (scroll, focus, hit-test) | ✅ implemented + tested |
| QUIC transport (Quinn, self-signed cert, Tailscale-trust) | ✅ implemented, loopback-verified |
| Session suspend → checkpoint → serve → resume → reconnect | ✅ implemented, loopback-verified |
| macOS capture (CGVirtualDisplay + SCK + VideoToolbox) | 🚧 stub (`simulboot-host/src/macos`) |
| Windows capture (WGC + NVENC/AMF) + Linux VM (HCS API) | 🚧 stub (`simulboot-host/src/windows`) |
| Metal renderer (`wgpu`) + VideoToolbox decode + winit input | 🚧 stub (`simulboot-client/src/render.rs`) |

The headless driver presents via `render::HeadlessRenderer`, which logs strip
state; the real Metal renderer and winit event loop plug into the same
`Renderer` trait and `Link` API.

## Build & test

```sh
cargo build --workspace
cargo test  --workspace
```

The default build compiles on any platform; native SDK dependencies are
commented in each crate's `Cargo.toml` and enabled per target as the backends
land.

## Try the loopback flow

```sh
# Terminal 1 — a host with the null capture backend (announce + input only)
cargo run -p simulboot-host -- \
  --bind 127.0.0.1:7001 --name Linux --os linux --capture display:0

# Terminal 2 — the compositor; Ctrl-C suspends and prints a resume URL
cargo run -p simulboot-client -- --host 127.0.0.1:7001 --out session.xml --broker-port 7000

# Terminal 3 — resume on a "second device" from the served image
cargo run -p simulboot-client -- \
  --resume http://127.0.0.1:7000/session/<id-from-terminal-2>
```

## Design notes

- **Wire protocol** — `StructureMessage` control enum on a QUIC reliable stream;
  `FrameHeader`-prefixed encoded frames on QUIC datagrams. bincode for v0
  (Cap'n Proto deferred to v1). A plain enum stands in for the Scribble
  multiparty session type (specified in the handoff).
- **Session image** — an XML Information Set in the
  `https://simulboot.dev/session/v1` namespace. Content-addressed by
  `sha256` of a deterministic canonical serialisation of the data model (the
  `<session>` id attribute is omitted from the hash to avoid circularity). Text
  XML for v0; Fast Infoset deferred.
- **Persistence telos (denotational scaffolding)** — the v0⇄v1 Galois connection
  in [`simulboot-common/src/galois.rs`](simulboot-common/src/galois.rs):
  `α` projects out the v1 coefficient namespace (timing × security × linearity,
  in [`coefficients.rs`](simulboot-common/src/coefficients.rs)) and `γ` reinstates
  it at the semiring top — the value v0 holds implicitly. Because the session id
  is computed over the base projection only, a v0 session survives concretisation
  to v1, loading into a future renderer, and re-checkpointing with a *stable
  hash* (laws L1–L3). The maps are specified language-agnostically in
  [`docs/persistence/`](docs/persistence/) (spec, XSDs, and a Python oracle); the
  Rust implementation answers that spec and is checked by property tests in
  [`tests/galois_laws.rs`](simulboot-common/tests/galois_laws.rs).
- **Surface uniformity (Claim B)** — every host implements one interface
  (`CaptureSource`: announce / frames / input). The compositor has no per-OS
  code paths.
- **OS as infrastructure (Claim C)** — on `Suspend` the host acks and keeps
  running; it reconnects to whichever compositor resumes the session.

## Licence

AGPL-3.0-or-later. See [`LICENSE`](LICENSE).
