# 0002 — Async runtime

- **Status:** Draft (proposed — awaiting ratification)
- **Skeletons:** none (architecture decision, not a placeholder)
- **Updated:** 2026-06-29
- **Summary:** drop full tokio in favour of `quinn` on its `runtime-smol`
  feature plus the smol / `async-io` stack; keep `quinn-proto` (sans-IO) in
  reserve for a future fused event loop.

## Context

The v0 spine runs on tokio: `#[tokio::main]`, `tokio::spawn`,
`tokio::sync::{mpsc, watch}`, `tokio::task::spawn_blocking`,
`tokio::time::timeout`, and quinn's default `runtime-tokio`. The workspace pulls
`tokio = { features = ["full"] }`.

For a low-level compositor whose real work is GPU, capture, and a thin slice of
networking, tokio is a heavy default. The actual async surface of the demo is
small:

- **Network** — drive QUIC and a couple of control channels.
- **Capture** — a blocking OS callback on a dedicated thread (`spawn_blocking`
  today; a plain `std::thread` serves equally).
- **Render** — a vsync loop under winit's event loop, which owns the main
  thread; wgpu is synchronous. Not an async task.
- **Input** — winit's event loop. Not tokio.

Tokio's headline features (work-stealing multithreaded scheduler; the
hyper/tonic ecosystem) go essentially unused. The broker's HTTP server is
hand-rolled, so no hyper dependency pulls tokio in either.

The unlock for this decision: **quinn 0.11 ships a `runtime-smol` feature**
(verified against the locked `quinn-0.11.11`). `src/runtime/async_io.rs` defines
`pub struct SmolRuntime` with a real `impl Runtime`, and `default_runtime()`
returns it when the feature is enabled. The smol and async-std runtimes share
the same `async-io` (epoll/kqueue/IOCP) reactor, so `runtime-smol` is the same
mature I/O machinery as the async-std runtime — just on smol's executor. Shedding
tokio therefore does **not** mean shedding quinn.

## Decision

Adopt **`quinn` with `runtime-smol`** as the async substrate:

- `quinn = { default-features = false, features = ["runtime-smol", "rustls-ring"] }`
  (plus whatever TLS features we already rely on), constructed with
  `SmolRuntime`:

  ```rust
  let socket = std::net::UdpSocket::bind(bind)?;
  let endpoint = quinn::Endpoint::new(
      quinn::EndpointConfig::default(),
      Some(server_config),
      socket,
      std::sync::Arc::new(quinn::SmolRuntime),
  )?;
  ```

- Executor and primitives via the smol family: `smol`/`async-io` for the
  reactor and timers, `async-channel` for mpsc-style channels, `async-net` for
  the broker's `TcpListener`, `futures-lite` for combinators (`timeout` becomes
  `async-io::Timer` + `or`).
- `tokio` is removed from the workspace. `simulboot-common` already has no
  runtime dependency and is unaffected.

This keeps quinn's ergonomic `Endpoint`/`Connection`/datagram API; only the
runtime underneath changes.

## Alternatives considered

1. **Stay on quinn + tokio (status quo).** Lowest effort, most mature, largest
   ecosystem — but most of that value is unused here, and `features = ["full"]`
   is wasteful regardless. Rejected as the default weight for a low-level demo;
   if kept, it should at minimum narrow tokio's features.
2. **`quinn-proto` + smol, sans-IO.** Drive the QUIC state machine directly: own
   the UDP pump, timer wheel, and loss/pacing timers. Maximum control and the
   smallest dependency, but it re-implements exactly what the quinn wrapper
   already provides. Its real payoff is *fusing* the QUIC loop into a single
   custom event loop alongside render/capture pacing — out of scope for the
   black triangle. **Deferred**, not discarded (see below).
3. **`quinn` + `runtime-smol` (chosen).** ~95% of the benefit of option 2 (no
   tokio, light async-io executor, winit-friendly) for a fraction of the work,
   because quinn still drives the protocol.

## Consequences

- No multithreaded work-stealing scheduler. For our workload this is fine and
  arguably an improvement next to winit (no parking a multithreaded runtime on
  the main thread).
- Two small ergonomic losses, both trivially covered: `spawn_blocking`
  (→ dedicated `std::thread`, natural for the blocking capture loop) and
  `tokio::sync::watch` (→ `async-watch` or an `event-listener`-based cell; used
  only for the suspend-ack signal in `conn.rs`).
- Slightly more explicit endpoint construction (`Endpoint::new` with a bound
  `UdpSocket` and the runtime), versus quinn's tokio convenience constructors.

## Migration scope

Bounded to the three binaries; `simulboot-common` untouched.

- **simulboot-host** — `main.rs`: `#[tokio::main]` → `smol::block_on`;
  `tokio::spawn` → `smol::spawn(_).detach()`; `spawn_blocking` (capture `run`)
  → `std::thread`; channels → `async-channel`. `net.rs`: `Endpoint::new` +
  `SmolRuntime`.
- **simulboot-client** — `main.rs`: entrypoint + present-loop timing
  (`tokio::time` → `async-io::Timer`). `conn.rs`: `mpsc`/`watch` → `async-channel`
  + watch replacement; `timeout` → `futures-lite`. `net.rs`: `SmolRuntime`.
- **simulboot-broker** — `TcpListener`/accept loop → `async-net` + smol;
  `main.rs` entrypoint.
- **Cargo** — drop `tokio`; add `smol`, `async-io`, `async-channel`, `async-net`,
  `futures-lite` (and a watch crate if needed) to the workspace; flip quinn to
  `default-features = false` + `runtime-smol`.

## Acceptance

- The loopback flow (connect → announce → strip → suspend → checkpoint → serve
  → resume → reconnect) passes with no tokio in the dependency tree
  (`cargo tree | grep -c tokio` is 0).
- `cargo build --workspace` and `cargo test --workspace` stay green;
  `cargo clippy --workspace --all-targets` stays at zero warnings.

## Deferred: quinn-proto sans-IO

Revisit option 2 if/when the renderer and network want a single unified event
loop — e.g. pacing frame delivery against vsync, or driving QUIC timers from the
same clock as the compositor. At that point the sans-IO `quinn-proto` state
machine becomes worth the extra plumbing. Until then, `runtime-smol` is the
correct altitude.
