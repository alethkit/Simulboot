//! The niri-style scrollable surface strip (F10–F13).
//!
//! Surfaces live on an infinite horizontal strip. New surfaces append to the
//! right; existing surfaces never resize when others arrive or leave. The
//! viewport is a fixed window `[scroll_pos, scroll_pos + viewport_width]` onto
//! that strip. All of this is pure geometry with no GPU or windowing
//! dependency, which is what makes it testable and what the session image
//! captures on suspend.
//!
//! Some accessors here are not yet called by the headless v0 driver; they are
//! the strip's intended public API, exercised by the unit tests and consumed by
//! the renderer and the winit input layer in build-order weeks 3 and 6.
#![allow(dead_code)]

use std::net::SocketAddr;

use simulboot_common::{Codec, HostProvenance, SurfaceId};

/// One surface placed on the strip.
#[derive(Debug, Clone)]
pub struct Surface {
    pub id: SurfaceId,
    pub name: String,
    /// Strip position, left to right, starting at 0.
    pub order: u32,
    /// Width/height in compositor pixels. Fixed once placed (F10).
    pub width: f32,
    pub height: f32,
    /// Native source dimensions, needed to denormalise input on the host side.
    pub source_width: u32,
    pub source_height: u32,
    pub codec: Codec,
    pub provenance: HostProvenance,
    pub host_addr: SocketAddr,
    /// PTS of the most recently received frame, if any (diagnostics only).
    pub last_pts: Option<u64>,
    /// Count of frames received on the content channel.
    pub frames_received: u64,
}

/// The strip plus viewport and scroll/focus state.
#[derive(Debug, Clone)]
pub struct Strip {
    surfaces: Vec<Surface>,
    scroll_pos: f32,
    viewport_width: f32,
    viewport_height: f32,
    focus: Option<SurfaceId>,
}

/// A surface hit by a viewport-space point, with the point expressed in that
/// surface's normalised 0.0–1.0 local space (ready for an [`InputEvent`]).
///
/// [`InputEvent`]: simulboot_common::InputEvent
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub id: SurfaceId,
    pub nx: f32,
    pub ny: f32,
}

impl Strip {
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        Strip {
            surfaces: Vec::new(),
            scroll_pos: 0.0,
            viewport_width,
            viewport_height,
            focus: None,
        }
    }

    /// The default width for a freshly placed surface: one-third of the viewport.
    pub fn default_surface_width(&self) -> f32 {
        self.viewport_width / 3.0
    }

    pub fn viewport_width(&self) -> f32 {
        self.viewport_width
    }

    pub fn viewport_height(&self) -> f32 {
        self.viewport_height
    }

    pub fn scroll_pos(&self) -> f32 {
        self.scroll_pos
    }

    pub fn focus(&self) -> Option<SurfaceId> {
        self.focus
    }

    pub fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }

    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    /// Append a surface at the right end. Its `order` is assigned to the current
    /// count; width defaults to one-third of the viewport and height to the full
    /// viewport height. Returns the assigned order.
    pub fn append(&mut self, mut surface: Surface) -> u32 {
        let order = self.surfaces.len() as u32;
        surface.order = order;
        if surface.width <= 0.0 {
            surface.width = self.default_surface_width();
        }
        if surface.height <= 0.0 {
            surface.height = self.viewport_height;
        }
        self.surfaces.push(surface);
        order
    }

    /// Insert a surface at a specific order (used when restoring a session image,
    /// where order is authoritative). Surfaces are kept sorted by `order`.
    pub fn insert_ordered(&mut self, surface: Surface) {
        self.surfaces.push(surface);
        self.surfaces.sort_by_key(|s| s.order);
    }

    /// Remove a surface by id. Remaining surfaces keep their relative order and
    /// are re-numbered so the strip closes the gap (F13). Clears focus if it was
    /// the removed surface. Returns the removed surface if present.
    pub fn remove(&mut self, id: &SurfaceId) -> Option<Surface> {
        let idx = self.surfaces.iter().position(|s| &s.id == id)?;
        let removed = self.surfaces.remove(idx);
        for (i, s) in self.surfaces.iter_mut().enumerate() {
            s.order = i as u32;
        }
        if self.focus == Some(*id) {
            self.focus = None;
        }
        self.clamp_scroll();
        Some(removed)
    }

    pub fn get(&self, id: &SurfaceId) -> Option<&Surface> {
        self.surfaces.iter().find(|s| &s.id == id)
    }

    pub fn get_mut(&mut self, id: &SurfaceId) -> Option<&mut Surface> {
        self.surfaces.iter_mut().find(|s| &s.id == id)
    }

    /// Record that a frame arrived for `id` (updates diagnostics).
    pub fn note_frame(&mut self, id: &SurfaceId, pts: u64) {
        if let Some(s) = self.get_mut(id) {
            s.last_pts = Some(pts);
            s.frames_received += 1;
        }
    }

    /// Left edge of a surface in strip space (cumulative width of predecessors).
    pub fn surface_left(&self, id: &SurfaceId) -> Option<f32> {
        let mut x = 0.0;
        for s in &self.surfaces {
            if &s.id == id {
                return Some(x);
            }
            x += s.width;
        }
        None
    }

    /// Total width of the strip's content.
    pub fn content_width(&self) -> f32 {
        self.surfaces.iter().map(|s| s.width).sum()
    }

    /// The furthest-right valid scroll position (never negative).
    pub fn max_scroll(&self) -> f32 {
        (self.content_width() - self.viewport_width).max(0.0)
    }

    /// Scroll by `dx` pixels (positive = rightward), clamped to `[0, max_scroll]`.
    /// Two-finger trackpad swipes drive this and are *not* forwarded to any host.
    pub fn scroll_by(&mut self, dx: f32) {
        self.scroll_pos += dx;
        self.clamp_scroll();
    }

    /// Set the scroll position directly (used on restore), clamped.
    pub fn set_scroll(&mut self, pos: f32) {
        self.scroll_pos = pos;
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        self.scroll_pos = self.scroll_pos.clamp(0.0, self.max_scroll());
    }

    /// Set focus to a specific surface if it exists.
    pub fn set_focus(&mut self, id: SurfaceId) -> bool {
        if self.surfaces.iter().any(|s| s.id == id) {
            self.focus = Some(id);
            true
        } else {
            false
        }
    }

    pub fn set_focus_opt(&mut self, id: Option<SurfaceId>) {
        self.focus = id.filter(|id| self.surfaces.iter().any(|s| &s.id == id));
    }

    /// Surfaces currently intersecting the viewport, in strip order.
    pub fn visible(&self) -> Vec<&Surface> {
        let view_l = self.scroll_pos;
        let view_r = self.scroll_pos + self.viewport_width;
        let mut x = 0.0;
        let mut out = Vec::new();
        for s in &self.surfaces {
            let l = x;
            let r = x + s.width;
            if r > view_l && l < view_r {
                out.push(s);
            }
            x = r;
        }
        out
    }

    /// Hit-test a viewport-space point. `vx`/`vy` are in viewport pixels (origin
    /// top-left). Returns the surface under the point and the point in that
    /// surface's normalised 0.0–1.0 space (F15). `None` if the point is past the
    /// last surface or outside the vertical band.
    pub fn hit_test(&self, vx: f32, vy: f32) -> Option<Hit> {
        if vy < 0.0 || vy > self.viewport_height {
            return None;
        }
        let strip_x = vx + self.scroll_pos;
        let mut x = 0.0;
        for s in &self.surfaces {
            let l = x;
            let r = x + s.width;
            if strip_x >= l && strip_x < r {
                let nx = (strip_x - l) / s.width;
                let ny = vy / s.height;
                return Some(Hit { id: s.id, nx, ny: ny.clamp(0.0, 1.0) });
            }
            x = r;
        }
        None
    }

    /// Click handling: focus whatever surface is under the point. Returns the hit
    /// (so the caller can also forward a normalised `MouseDown` if desired).
    pub fn click(&mut self, vx: f32, vy: f32) -> Option<Hit> {
        let hit = self.hit_test(vx, vy)?;
        self.focus = Some(hit.id);
        Some(hit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simulboot_common::OsKind;

    fn provenance() -> HostProvenance {
        HostProvenance {
            os: OsKind::Linux,
            machine_name: "test".into(),
            tailscale_addr: "127.0.0.1".into(),
            capture_description: "display:0".into(),
        }
    }

    fn surface(id: u8) -> Surface {
        Surface {
            id: [id; 32],
            name: format!("s{id}"),
            order: 0,
            width: 0.0, // let append assign the default
            height: 0.0,
            source_width: 1920,
            source_height: 1080,
            codec: Codec::H265,
            provenance: provenance(),
            host_addr: "127.0.0.1:7001".parse().unwrap(),
            last_pts: None,
            frames_received: 0,
        }
    }

    fn strip3() -> Strip {
        // viewport 900 wide -> default surface width 300.
        let mut s = Strip::new(900.0, 600.0);
        s.append(surface(1));
        s.append(surface(2));
        s.append(surface(3));
        s
    }

    #[test]
    fn append_assigns_order_and_default_size() {
        let s = strip3();
        assert_eq!(s.len(), 3);
        assert_eq!(s.surfaces()[0].order, 0);
        assert_eq!(s.surfaces()[2].order, 2);
        assert_eq!(s.surfaces()[0].width, 300.0);
        assert_eq!(s.surfaces()[0].height, 600.0);
        assert_eq!(s.content_width(), 900.0);
    }

    #[test]
    fn surface_left_is_cumulative() {
        let s = strip3();
        assert_eq!(s.surface_left(&[1; 32]), Some(0.0));
        assert_eq!(s.surface_left(&[2; 32]), Some(300.0));
        assert_eq!(s.surface_left(&[3; 32]), Some(600.0));
    }

    #[test]
    fn scroll_clamps_to_content() {
        let mut s = strip3();
        // content 900 == viewport 900 -> max scroll 0.
        s.scroll_by(500.0);
        assert_eq!(s.scroll_pos(), 0.0);
        s.append(surface(4)); // content now 1200, max scroll 300
        s.scroll_by(500.0);
        assert_eq!(s.scroll_pos(), 300.0);
        s.scroll_by(-1000.0);
        assert_eq!(s.scroll_pos(), 0.0);
    }

    #[test]
    fn hit_test_normalises_within_surface() {
        let s = strip3();
        // Point at viewport x=150 (middle of surface 1, width 300), y=300 of 600.
        let hit = s.hit_test(150.0, 300.0).unwrap();
        assert_eq!(hit.id, [1; 32]);
        assert!((hit.nx - 0.5).abs() < 1e-6);
        assert!((hit.ny - 0.5).abs() < 1e-6);
    }

    #[test]
    fn hit_test_accounts_for_scroll() {
        let mut s = strip3();
        s.append(surface(4)); // max scroll 300
        s.scroll_by(300.0); // surface 2 now starts at viewport x=0
        let hit = s.hit_test(0.0, 0.0).unwrap();
        assert_eq!(hit.id, [2; 32]);
    }

    #[test]
    fn click_sets_focus() {
        let mut s = strip3();
        let hit = s.click(450.0, 100.0).unwrap(); // x 450 -> surface 2 (300..600)
        assert_eq!(hit.id, [2; 32]);
        assert_eq!(s.focus(), Some([2; 32]));
    }

    #[test]
    fn remove_closes_gap_and_clears_focus() {
        let mut s = strip3();
        s.set_focus([2; 32]);
        s.remove(&[2; 32]);
        assert_eq!(s.len(), 2);
        assert_eq!(s.focus(), None);
        // surface 3 re-numbered to order 1 and shifted left to x=300.
        assert_eq!(s.get(&[3; 32]).unwrap().order, 1);
        assert_eq!(s.surface_left(&[3; 32]), Some(300.0));
    }

    #[test]
    fn visible_reflects_viewport_window() {
        let mut s = strip3();
        s.append(surface(4)); // 4 surfaces, 300 each, content 1200
        // viewport 900 at scroll 0 -> surfaces 1,2,3 visible.
        let vis: Vec<_> = s.visible().iter().map(|x| x.id[0]).collect();
        assert_eq!(vis, vec![1, 2, 3]);
        s.scroll_by(300.0); // window [300,1200] -> surfaces 2,3,4
        let vis: Vec<_> = s.visible().iter().map(|x| x.id[0]).collect();
        assert_eq!(vis, vec![2, 3, 4]);
    }
}
