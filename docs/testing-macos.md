# Testing the macOS side

The macOS capture backend (`simulboot-host/src/macos`) can only be exercised on
a real Mac with a logged-in GUI session and granted permissions — headless CI
and the Linux dev environment cannot do live screen capture. Testing therefore
splits three ways:

| Tier | Where | What it covers |
| --- | --- | --- |
| Compile + unit | GitHub `macos-latest` CI (free, public repo) | the `cfg(target_os = "macos")` code builds; SDK-free logic (announce geometry, format/codec mapping) passes |
| Live capture / encode / inject | **this runbook, on your Mac** | real frames flow; input reaches the captured app |
| Cross-platform spine | Linux CI | trait, wire, session, strip, persistence |

The first two are the macOS story; this document is the second.

## Prerequisites

- A Mac with a logged-in desktop session (not over plain SSH — screen capture
  needs a real window server).
- A Rust toolchain (`rustup`, stable).
- macOS 12.3+ (ScreenCaptureKit).

## Permissions (the part that bites)

The host binary needs two TCC permissions. The grant attaches to **whatever
launches it** — your terminal app (Terminal/iTerm) when run from a shell, or the
binary itself once it has a stable path.

1. **Screen Recording** — required for ScreenCaptureKit to return frames.
   System Settings → Privacy & Security → **Screen Recording** → enable your
   terminal app. The first capture attempt also triggers the system prompt.
2. **Accessibility** — required for `CGEventPost` input injection.
   System Settings → Privacy & Security → **Accessibility** → enable your
   terminal app.

After granting, fully quit and relaunch the terminal (TCC changes are picked up
on relaunch).

## The loopback (host → compositor, both on the Mac)

Two terminals on the same machine:

```sh
# Terminal 1 — the macOS host capturing a window/display
cargo run -p simulboot-host -- \
  --bind 127.0.0.1:7001 --name macOS --os macos --capture window:Safari

# Terminal 2 — the compositor
cargo run -p simulboot-client -- --host 127.0.0.1:7001 --out session.xml --broker-port 7000
```

What to look for, in order:
- host log: `compositor connected`, then capture start (no `using NullCapture`
  warning once the real backend is in).
- client log: `surface added … macOS`, then recurring frame-arrival lines from
  the present loop (`render: …`) — that is "frames are flowing," observable
  before the real renderer exists.
- press a key with the captured app focused via the compositor → it should reach
  the app (Accessibility working).
- Ctrl-C in Terminal 2 → suspends, writes `session.xml`, prints a resume URL
  (host stays alive — Claim C).

## Current state

Until the macOS backend lands (`SKELETONS.md` → "Platform capture backends";
plan `docs/plans/0001-capture-backends.md`), `build_source` returns
`NullCapture`: the loopback completes the announce / suspend / resume handshakes
but **emits no frames**. So today this runbook validates the connection and
session path on macOS; the frame-arrival and input steps light up as the backend
is implemented, one slice at a time.
