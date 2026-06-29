//! The single host interface (F9).
//!
//! Every host — whatever its OS or capture mechanism — implements
//! [`CaptureSource`]. The host runtime in `main.rs` is written entirely against
//! this trait and knows nothing about ScreenCaptureKit, WGC, or PipeWire. That
//! is what keeps the compositor free of per-OS code paths (Claim B): uniformity
//! is enforced at the host boundary.
//!
//! `CaptureSource::announce` is part of the host interface (F9); the v0 driver
//! sources the announce from CLI config instead, so it is unused until a real
//! backend derives it from actual capture geometry.
#![allow(dead_code)]

use anyhow::Result;
use simulboot_common::{InputEvent, SurfaceAnnounce};

/// One encoded frame ready for the content channel.
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    /// Presentation timestamp, in the source's own clock units.
    pub pts: u64,
    /// The encoded bitstream (H.265 for v0).
    pub bytes: Vec<u8>,
}

/// Channel the source pushes encoded frames into; the runtime forwards them onto
/// the QUIC content (datagram) channel.
pub type FrameSink = async_channel::Sender<EncodedFrame>;

/// Channel the runtime pushes decoded input events into; the source injects them
/// into the source OS.
pub type InputStream = async_channel::Receiver<InputEvent>;

/// A capture/encode/inject backend for one surface.
///
/// The natural shape for a native backend: a capture callback fires on the OS's
/// thread, hands an `IOSurface`/`IDirect3DSurface` to the hardware encoder, and
/// pushes the encoded bytes into `frames`. Input arrives on `input` and is
/// injected (`CGEventPost`, `SendInput`, …). Because that callback is blocking,
/// [`run`](CaptureSource::run) executes on a dedicated blocking task — it may
/// block freely.
pub trait CaptureSource: Send + 'static {
    /// Describe the surface this source produces. Called at connect and again on
    /// reconnect (the host re-announces with a fresh frame).
    fn announce(&self) -> SurfaceAnnounce;

    /// Run the capture loop until the source ends or both channels close.
    /// `frames` is closed by the runtime when the connection drops; `input`
    /// yields `None` when the runtime stops forwarding input.
    fn run(self: Box<Self>, frames: FrameSink, input: InputStream) -> Result<()>;
}

/// A capture source that announces a surface but produces no frames.
///
/// This is the week-1 stand-in: it lets the QUIC connection, the announce
/// handshake, and the input path be exercised end-to-end before any real
/// capture backend exists. It is also the fallback on platforms whose native
/// backend is not yet implemented.
pub struct NullCapture {
    announce: SurfaceAnnounce,
}

impl NullCapture {
    pub fn new(announce: SurfaceAnnounce) -> Self {
        NullCapture { announce }
    }
}

impl CaptureSource for NullCapture {
    fn announce(&self) -> SurfaceAnnounce {
        self.announce.clone()
    }

    fn run(self: Box<Self>, _frames: FrameSink, input: InputStream) -> Result<()> {
        // No capture backend: emit nothing, but stay alive draining input so the
        // routing path can be tested. Returns when the runtime closes `input`.
        while let Ok(event) = input.recv_blocking() {
            tracing::debug!(?event, "NullCapture: dropping input (no backend)");
        }
        Ok(())
    }
}
