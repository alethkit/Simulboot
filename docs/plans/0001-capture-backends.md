# 0001 — Capture backends

- **Status:** Draft
- **Skeletons:** `SKELETONS.md` → "Platform capture backends"
  (`simulboot-host/src/{macos,windows,linux}`)
- **Updated:** 2026-06-29
- **Summary:** implement real `CaptureSource` backends — capture a surface,
  hardware-encode it, inject input — one OS at a time, all behind the existing
  trait.

## Goal

Each platform module's `build_source` returns a backend that, given a
`SurfaceAnnounce`, captures the described surface, encodes frames (H.265 for v0),
pushes them onto the `FrameSink`, and injects inbound events from the
`InputStream` into the source OS. The compositor is unchanged: it already drives
everything through `CaptureSource` and `Link`, so a real backend is a drop-in for
`NullCapture`.

The trait contract is already fixed in `simulboot-host/src/capture.rs`:

```rust
trait CaptureSource: Send + 'static {
    fn announce(&self) -> SurfaceAnnounce;
    fn run(self: Box<Self>, frames: FrameSink, input: InputStream) -> Result<()>;
}
```

`run` executes on a dedicated blocking task, so a native capture callback may
block freely.

## Approach

Build **one OS end to end first**, prove the trait shape against real hardware,
then replicate. Each backend is the same three-stage pipeline:

| Stage | macOS | Windows | Linux (VM) |
| --- | --- | --- | --- |
| Capture | CGVirtualDisplay + ScreenCaptureKit | Windows.Graphics.Capture | PipeWire |
| Encode | VideoToolbox (HW H.265) | NVENC / AMF | VA-API |
| Inject input | `CGEventPost` | `SendInput` | `uinput` / VM channel |

The natural shape (per the `capture.rs` module docs): a capture callback fires on
the OS's thread, hands an `IOSurface`/`IDirect3DSurface`/`DmaBuf` straight to the
hardware encoder, and the encoded bytes go onto `frames`. Input arrives on
`input` and is injected. Zero-copy from capture to encoder where the platform
allows it.

Per-OS native dependencies are already stubbed (commented) in
`simulboot-host/Cargo.toml`; enable them under their `#[cfg(target_os = …)]`
gates so the default cross-platform build keeps compiling.

## Open questions

- **Which OS first?** The compositor runs on the demo MacBook, so the macOS host
  is the lowest-friction path (same machine, no second device, well-documented
  SCK + VideoToolbox). Confirm against `SIMULBOOT_HANDOFF.md` before committing.
- **Frame sizing vs. transport.** Real encoded frames will exceed the QUIC
  datagram MTU; this plan's frames are what force the fragmentation work
  (Phase C). Decide whether to land a minimal fragmentation shim here or keep
  resolutions small until Phase C.
- **`announce` geometry.** v0 sources the announce from CLI config; a real
  backend should derive width/height/codec from the actual capture. Decide when
  to switch `CaptureSource::announce` from config-driven to capture-driven.
- **Encoder availability.** NVENC/AMF and VA-API depend on host hardware; define
  the fallback (software encode? refuse?) when absent.

## Risks

- Native SDK surface is large and platform-locked; CI cannot exercise these on a
  single runner, so each backend is validated manually on its OS.
- Color space / pixel format mismatches between capture and encoder are a classic
  time sink.
- Keeping the work behind the trait is essential — any platform type leaking into
  the compositor breaks Claim B and must be caught in review.

## Acceptance

- On the first OS: run `simulboot-host` with the real backend, connect the
  compositor, and see a live capture of the announced surface composited in the
  strip; injected keystrokes reach the captured application.
- The default `cargo build --workspace` (no target SDK) still compiles via the
  `NullCapture` fallback.
- Delete the corresponding `SKELETONS.md` entry as each OS lands; mark this plan
  `Done` when all three do.
