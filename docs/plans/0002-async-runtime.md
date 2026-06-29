# 0002 — Async runtime

- **Status:** Accepted (2026-06-29) — implemented on the code branch (`83196bd`)
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
- `tokio` is removed as a *direct* dependency of every crate. It remains in the
  tree only as quinn's own non-optional leaf (see Consequences).
  `simulboot-common` already has no runtime dependency and is unaffected.

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
   tokio *runtime*, light async-io executor, winit-friendly) for a fraction of
   the work, because quinn still drives the protocol. Caveat learned in
   implementation: quinn keeps a `tokio/sync` leaf (see Consequences); only
   option 2 removes `tokio` entirely.

## Consequences

- **tokio is not fully removable** (corrected after implementation). quinn
  0.11's `[dependencies.tokio]` is *non-optional* with `features = ["sync"]`, so
  `tokio` stays in the tree as a quinn-internal leaf regardless of runtime. The
  build activates only its `sync` (+ empty `default`) feature — **no `rt`,
  `net`, `time`, macros, scheduler, or signal handling**. What this ADR removes
  is the tokio *runtime*, not the `tokio` crate. Dropping the leaf too would
  mean dropping quinn (i.e. option 2, `quinn-proto` sans-IO).
- No multithreaded work-stealing scheduler. For our workload this is fine and
  arguably an improvement next to winit (no parking a multithreaded runtime on
  the main thread).
- Two small ergonomic losses, both trivially covered: `spawn_blocking`
  (→ `smol::unblock`, natural for the blocking capture loop) and
  `tokio::sync::watch` (→ a bounded(1) `async-channel`; used only for the
  suspend-ack signal in `conn.rs`).
- No built-in signal handling (tokio had `signal::ctrl_c`); Ctrl-C is handled
  via a `ctrlc` handler fanned out over a channel.
- Slightly more explicit endpoint construction (`Endpoint::new` with a bound
  `UdpSocket` and the runtime), versus quinn's tokio convenience constructors.

## Migration scope

Bounded to the three binaries; `simulboot-common` untouched.

As implemented (`83196bd` on the code branch):

- **simulboot-host** — `main.rs`: `#[tokio::main]` → `smol::block_on`;
  `tokio::spawn` → `smol::spawn(_).detach()`; `spawn_blocking` (capture `run`)
  → `smol::unblock`; channels → `async-channel` (`recv_blocking` on the capture
  thread). `net.rs`: `Endpoint::new` + `SmolRuntime`.
- **simulboot-client** — `main.rs`: entrypoint → `smol::block_on`; present-loop
  timing → `smol::Timer`; Ctrl-C → `ctrlc` handler over a channel. `conn.rs`:
  `mpsc` → `async-channel`, the suspend-ack `watch` → a bounded(1)
  `async-channel`, `timeout` → `smol::future::or` + `smol::Timer`. `net.rs`:
  `SmolRuntime`.
- **simulboot-broker** — `TcpListener`/accept loop → `smol::net` + `smol::io`,
  `select!` → `smol::future::or`; `main.rs` entrypoint + `ctrlc`.
- **Cargo** — drop direct `tokio`; add `smol`, `async-channel`, `ctrlc` (smol
  re-exports async-io / async-net / futures-lite, so no separate entries); flip
  quinn to `default-features = false` + `["runtime-smol", "rustls-ring"]`.

## Acceptance

- The loopback flow (connect → announce → strip → suspend → checkpoint → serve)
  runs on the smol runtime — verified by hand: host accepts, compositor
  connects, SIGINT triggers `1/1 hosts acknowledged suspend`, and a valid
  content-addressed `session.xml` is written and served.
- No tokio **runtime** in the tree: `cargo tree -e features -i tokio` shows only
  the `sync` (+ empty `default`) feature, pulled by quinn alone — no `rt`/`net`/
  `time`/macros, and no direct dependency from our crates.
- `cargo build --workspace`, `cargo test --workspace` (52 tests), and
  `cargo clippy --workspace --all-targets` all stay green.

## Deferred: quinn-proto sans-IO

Revisit option 2 if/when the renderer and network want a single unified event
loop — e.g. pacing frame delivery against vsync, or driving QUIC timers from the
same clock as the compositor. At that point the sans-IO `quinn-proto` state
machine becomes worth the extra plumbing. Until then, `runtime-smol` is the
correct altitude.
