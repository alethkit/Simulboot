# CLAUDE.md

Guidance for working in this repository. Read this, then `README.md` for the
project pitch and `docs/denotation.md` for what the substrate *means*.

## What this is

Simulboot is a proof-by-construction counter-argument to Kell's "an OS is
necessary": the thing the user inhabits — the **session** — is separable from any
OS instance, portable across machines, and self-describing. The v0 demo runs a
compositor that displays surfaces captured from macOS, Windows, and a Linux VM,
then **suspends** the session to a content-addressed image and **resumes** it on
another device while the source OS instances keep running.

This repo implements the v0 scope in `SIMULBOOT_HANDOFF.md` and nothing beyond
it. v1 is a future tightening, not a rewrite (see "Invariants" below and
`docs/denotation.md §7`).

## Workspace

A 2021-edition Cargo workspace, `resolver = "2"`, `rust-version = 1.75`. Four
crates, dependency flow `common → {host, client, broker}`:

- `simulboot-common` — shared types only: the QUIC wire protocol (`wire`), the
  session image (`session`), and the v0⇄v1 persistence layer (`coefficients`,
  `galois`). No platform code, no clock (kept deterministic and dependency-light).
- `simulboot-host` — runs on each source machine; captures one surface and
  serves it over QUIC. All OS-specific code lives here.
- `simulboot-client` — the compositor: strip layout, rendering, session
  suspend/resume.
- `simulboot-broker` — a minimal HTTP server that parks a suspended session
  image for the resuming device to fetch.

## Build, test, lint

```sh
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets   # the bar: zero warnings
```

The default build compiles on any platform. Native SDK dependencies are
commented in each crate's `Cargo.toml` and enabled per target as backends land,
so the cross-platform spine always builds.

Loopback demo (three terminals): a `--name`/`--os` host with the null capture
backend, the client (Ctrl-C suspends and prints a resume URL), then a second
client invocation with `--resume <url>`. Exact commands are in `README.md`.

## Invariants — must hold in v0, must survive into v1

These are not style preferences; they are the load-bearing structure. Breaking
one breaks the thesis or the v0→v1 refinement.

- **Surface uniformity (Claim B): the compositor has no per-OS code.** Every host
  implements the single `CaptureSource` trait (`announce` / frames / input). All
  OS/SDK access is confined to `simulboot-host/src/{macos,windows,linux}`;
  everything else speaks only `CaptureSource` and the client's `Link` API. This
  is our version of "external deps go through a wrapper" — keep the boundary
  clean rather than threading platform types through the compositor.
- **OS as infrastructure (Claim C):** on `Suspend` the host acks and *keeps
  running*, ready to reconnect to whichever compositor resumes the session.
- **`SurfaceId` is a content hash, never a human-readable name.** It is the seed
  of the persistence projection.
- **Session identity is base-determined.** `session_id = SHA256(C14N(base
  projection))`; the v1 coefficient block is excluded from the hash, so a v1
  image and its α share an id (`docs/persistence/galois-connection.md`, law L1).
- **Wire messages stay typed** (`StructureMessage`). The v1 multiparty session
  type layers on additively; do not regress to untyped framing.

## Standards

- Edition 2021; pinned `rust-version = 1.75`.
- Errors: `thiserror` for library error enums (`SessionError`, `WireError`),
  `anyhow` at binary/IO boundaries.
- Transport: `quinn` (QUIC) + `rustls` (ring) + `rcgen` self-signed cert;
  Tailscale provides the real authentication, not the TLS cert.
- Serialisation: `bincode` behind a 4-byte length prefix for control frames;
  `quick-xml` for the session image; `sha2`/`hex` for content addressing.
- `simulboot-common` is clock-free: timestamps are passed in as RFC-3339
  strings, never read from a system clock inside the crate.
- Scaffold APIs not yet called by the headless v0 driver carry
  `#[allow(dead_code)]` with a comment naming the build-order step that will use
  them. Do not delete them to silence the lint.

## Skeletons

Intentional placeholders — stubs, deferred subsystems, dropped behaviour — are
tracked in `SKELETONS.md`. **When you leave a skeleton, add an entry; when you
finish one, delete its entry.** It is a live TODO ordered by impact, not a
changelog. Check it before starting work so you do not "discover" a gap that is
already known and scoped.

## Documentation map

- `SIMULBOOT_HANDOFF.md` — the v0 build brief and the prose Kell rebuttal.
- `docs/denotation.md` — the theoretical spine: one denotation, v0 and v1 as the
  same thing at different coefficients.
- `docs/persistence/` — how a saved session survives a renderer upgrade (the
  α/γ Galois connection, the two XSDs, the Python oracle).
- `DEPENDENCIES.md` — every external crate and why it is in the tree (including
  the transitive `tokio/sync` leaf via quinn).
- `alternatives.md` — close-call tech choices to revisit, each with the trigger
  that should prompt it.
- `docs/testing-macos.md` — how the macOS capture backend is tested (CI compile
  lane + the manual live-capture loopback and its permissions).

## Conventions for changes

- Match the surrounding code's comment density and idiom; the crates are heavily
  doc-commented by design (each module opens with *why*, not just *what*).
- Keep `cargo clippy --workspace --all-targets` at zero warnings.
- Commit messages: imperative subject, a body explaining the *why*. Do not put
  model identifiers in commits, code, or any pushed artifact.
