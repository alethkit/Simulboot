//! Linux capture backend (stub).
//!
//! Per the brief, a bare-metal Linux host is *not* required for the v0 demo (the
//! Linux surface is produced by a Hyper-V VM captured via WGC on the Windows
//! host — see [`crate::windows`]). This module is the future native path:
//! **PipeWire** screencast portal for capture + **VA-API** for hardware encode,
//! with input injection via `uinput`/libei.

use anyhow::Result;
use simulboot_common::SurfaceAnnounce;

use crate::capture::{CaptureSource, NullCapture};

/// Build the Linux capture source for `announce`.
///
/// Not yet implemented (and not on the demo's critical path); returns a
/// [`NullCapture`].
pub fn build_source(announce: SurfaceAnnounce) -> Result<Box<dyn CaptureSource>> {
    tracing::warn!("Linux capture backend not implemented; using NullCapture");
    Ok(Box::new(NullCapture::new(announce)))
}
