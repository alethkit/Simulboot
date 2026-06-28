//! macOS capture backend (stub).
//!
//! Target pipeline (build-order week 2):
//!
//! * **CGVirtualDisplay** — created in a `vd-helper` subprocess so it survives a
//!   host crash and is unaffected by lid open/close. Declare 27" physical
//!   dimensions (597×336mm) to stay under the PPI cap.
//! * **ScreenCaptureKit** — `SCContentSharingPicker` for consented selection;
//!   frames arrive as IOSurface-backed `CMSampleBuffer`s.
//! * **VideoToolbox** — `VTCompressionSession` H.265, fed the IOSurface zero-copy.
//! * **CGEventPost(kCGHIDEventTap, …)** — input injection; denormalise mouse
//!   coordinates to the virtual display's pixel space.
//! * **IOPMAssertion** — `PreventUserIdleSystemSleep` so lid-close does not sleep
//!   the machine mid-session.

use anyhow::Result;
use simulboot_common::SurfaceAnnounce;

use crate::capture::{CaptureSource, NullCapture};

/// Build the macOS capture source for `announce`.
///
/// Not yet implemented; returns a [`NullCapture`] so the host still establishes
/// connections and exercises the announce/input paths.
pub fn build_source(announce: SurfaceAnnounce) -> Result<Box<dyn CaptureSource>> {
    tracing::warn!("macOS capture backend not implemented; using NullCapture");
    Ok(Box::new(NullCapture::new(announce)))
}
