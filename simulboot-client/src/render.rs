//! Rendering abstraction.
//!
//! The compositor's strip/session/input logic is render-agnostic; it drives a
//! [`Renderer`]. The v0 demo target is a `wgpu` Metal renderer (build-order
//! week 3) that, at vsync, iterates the visible surfaces in strip order and
//! composites each decoded frame (an IOSurface-backed `MTLTexture`) into the
//! render pass. That backend lives behind the commented `wgpu`/`winit`
//! dependencies in `Cargo.toml`.
//!
//! Until then, [`HeadlessRenderer`] lets the whole pipeline run, log, and be
//! tested without a window or GPU.

use crate::strip::Strip;

/// A presenter for the strip. Implementations own GPU textures keyed by
/// `SurfaceId` and upload decoded frames into them out of band; [`present`]
/// composites whatever is current.
///
/// [`present`]: Renderer::present
pub trait Renderer {
    /// Composite and present one frame of the strip's current visible state.
    fn present(&mut self, strip: &Strip);
}

/// A renderer that draws nothing and only logs strip state. Useful for headless
/// runs and tests of the surrounding pipeline.
#[derive(Default)]
pub struct HeadlessRenderer {
    last_log: Option<String>,
}

impl HeadlessRenderer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Renderer for HeadlessRenderer {
    fn present(&mut self, strip: &Strip) {
        // Only log when the visible composition changes, to avoid per-vsync spam.
        let summary = strip
            .visible()
            .iter()
            .map(|s| {
                let focused = strip.focus() == Some(s.id);
                format!("{}{}", s.name, if focused { "*" } else { "" })
            })
            .collect::<Vec<_>>()
            .join(" | ");
        let line = format!("[{:>6.0}px] {summary}", strip.scroll_pos());
        if self.last_log.as_deref() != Some(line.as_str()) {
            tracing::info!("strip: {line}");
            self.last_log = Some(line);
        }
    }
}
