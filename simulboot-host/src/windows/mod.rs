//! Windows capture backend (stub) + Linux-VM host.
//!
//! Target pipeline (build-order weeks 4–5):
//!
//! * Verify Session 1 at startup (`WTSGetActiveConsoleSessionId() == 1`); WGC
//!   fails in Session 0.
//! * **WGC** (`Windows.Graphics.Capture`) — `GraphicsCapturePicker` for consent;
//!   frames as `IDirect3DSurface` GPU textures; disable the yellow border via
//!   `IGraphicsCaptureSession3::IsBorderRequired = false` on 11 22H2+.
//! * **Hardware encode** — NVENC / AMF / Quick Sync depending on detected GPU,
//!   fed the `IDirect3DSurface` zero-copy.
//! * **`SendInput`** — input injection for native surfaces.
//! * **HCS API** (`computecore.dll`) — boot a Hyper-V Gen-2 VM from the physical
//!   Linux SSD (NanaBox-style JSON config). Capture the VM window with WGC, same
//!   path as native surfaces; runs on port 7002. Inject input via the VM channel.

use anyhow::Result;
use simulboot_common::SurfaceAnnounce;

use crate::capture::{CaptureSource, NullCapture};

/// Build the Windows (or Linux-VM-on-Windows) capture source for `announce`.
///
/// Not yet implemented; returns a [`NullCapture`].
pub fn build_source(announce: SurfaceAnnounce) -> Result<Box<dyn CaptureSource>> {
    tracing::warn!("Windows capture backend not implemented; using NullCapture");
    Ok(Box::new(NullCapture::new(announce)))
}
