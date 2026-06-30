# DEPENDENCIES

Every external crate in the workspace, and why it's here. The bar for adding a
dependency is that it earns its place; this file is the record of that
justification. Keep it current when you add or drop a crate.

## Principles

- **`simulboot-common` stays light and clock-free.** It carries only the shared
  types (wire protocol, session image, persistence layer). No runtime, no
  network, no system clock — timestamps are passed in as strings so the crate is
  deterministic and trivially testable.
- **Platform/SDK code is confined to `simulboot-host`.** Native capture/encode
  dependencies are declared per-target and commented until a backend lands, so
  the default build is cross-platform (Claim B: the compositor has no per-OS
  code).
- **Pinned floor:** edition 2021, `rust-version = 1.75`.
- Versions are declared once in `[workspace.dependencies]` (root `Cargo.toml`)
  and referenced per crate with `name.workspace = true`.

## Workspace-internal

| Crate | Why |
| --- | --- |
| `simulboot-common` | The shared type crate; depended on by all three binaries. |
| `simulboot-broker` | The client links it directly to serve a checkpoint in-process, rather than shelling out to the broker binary. |

## Serialisation & content addressing

Used by `simulboot-common` (and the wire/datagram paths in host/client).

| Crate | Ver | Why |
| --- | --- | --- |
| `serde` | 1 | Derive `Serialize`/`Deserialize` for the wire types (`StructureMessage`, `FrameHeader`). |
| `postcard` | 1 | Compact serde binary encoding for control frames (behind a 4-byte length prefix) and datagram headers. Replaces bincode, which is unmaintained ([RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141)); postcard has a documented, stable wire spec — right for a versioned protocol. Cap'n Proto is the v1 item. See [`alternatives.md`](alternatives.md) for the bitcode option. |
| `quick-xml` | 0.31 | Read/write the session image (XML Information Set). Event-based, no DOM allocation. |
| `sha2` | 0.10 | SHA-256 for content addressing (`SurfaceId`, `session_id`). |
| `hex` | 0.4 | Render/parse the `sha256:<hex>` form of content hashes. |

No `chrono`/`time`: RFC-3339 timestamps are formatted by a hand-rolled
`civil_from_days` in the client (`session/mod.rs`), keeping `common` clock-free.

## Async runtime & transport

The runtime choice (smol over tokio) is recorded in
[`docs/plans/0002-async-runtime.md`](docs/plans/0002-async-runtime.md).

| Crate | Ver | Used by | Why |
| --- | --- | --- | --- |
| `quinn` | 0.11 | host, client | QUIC transport (streams + datagrams). Built with `default-features = false, features = ["runtime-smol", "rustls-ring"]` so it runs on smol, not tokio. |
| `smol` | 2 | host, client, broker | The async runtime: `block_on`, `spawn`, `Timer`, `unblock`, `lock::Mutex`, `net`, `io`, `future::or`. Re-exports async-io / async-net / futures-lite, so those need no separate entries. |
| `async-channel` | 2 | host, client | mpsc-style channels (control, frames, input, suspend-ack). `recv_blocking` lets the blocking capture thread drain input. |
| `ctrlc` | 3 | client, broker | SIGINT handling — smol has none. The handler fans out over a channel (suspend on first Ctrl-C, stop the broker on the next). |
| `rustls` | 0.23 | host, client | TLS for QUIC (`ring` provider, `std`). Tailscale is the real authenticator; the cert just satisfies the handshake. |
| `rcgen` | 0.13 | host | Generate the host's ephemeral self-signed certificate at startup. |
| `bytes` | 1 | host | `Bytes` for zero-copy datagram payloads handed to quinn. |

## Diagnostics & CLI

| Crate | Ver | Why |
| --- | --- | --- |
| `anyhow` | 1 | Error handling at binary/IO boundaries (`Result<()>` in mains and drivers). |
| `thiserror` | 1 | Typed library error enums in `common` (`SessionError`, `WireError`). |
| `tracing` | 0.1 | Structured logging throughout. |
| `tracing-subscriber` | 0.3 | The `fmt` subscriber + `env-filter` (`RUST_LOG`) in each binary. |

## Testing (dev-dependencies)

| Crate | Ver | Why |
| --- | --- | --- |
| `proptest` | 1 | Property tests for the persistence Galois-connection laws (`simulboot-common/tests/galois_laws.rs`): generators for `I₀`/`I₁` images, checking L1–L3. |

## Notable transitive dependencies

Worth knowing because the names appear in `cargo tree`:

- **`tokio` (sync only) — via `quinn`.** quinn's `[dependencies.tokio]` is
  *non-optional* with `features = ["sync"]`, so `tokio` is in the tree no matter
  which runtime feature we pick. Only `tokio::sync` is compiled — `Notify`,
  `mpsc`, `oneshot`: lock-free async primitives quinn uses internally to wake its
  `Connection`/`SendStream` handles. **No `rt`, `net`, `time`, macros, or
  scheduler** are enabled, so there is no tokio *runtime* in the binary, just a
  small primitives library. Removing even this would mean dropping quinn for
  sans-IO `quinn-proto` (see ADR 0002, "Deferred"). Verify with
  `cargo tree -e features -i tokio`.
- **`async-io` — under `smol`.** The epoll/kqueue/IOCP reactor that actually
  drives the UDP socket and timers; the smol and async-std quinn runtimes share
  it.

## Platform-native backends (commented, per-target)

Declared but commented in `simulboot-host/Cargo.toml` and
`simulboot-client/Cargo.toml`, enabled per target as backends land
(`SKELETONS.md` tracks the work):

- **macOS host:** `screencapturekit`, `objc2`, `core-foundation`,
  `core-graphics`, `ffmpeg-next` (CGVirtualDisplay + SCK capture, VideoToolbox
  encode).
- **Windows host:** `windows` (WGC capture + D3D11/DXGI; HCS API for the Linux
  VM).
- **client renderer:** `wgpu`, `winit`, `ffmpeg-next` (Metal compositor +
  VideoToolbox decode + real input).

These stay out of the default build so the cross-platform spine always compiles.
