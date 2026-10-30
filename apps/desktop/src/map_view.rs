// SPDX-License-Identifier: GPL-3.0-or-later

//! The map view (P5-05, MAP-17): a BSP drawn by `qs-render` inside the window, through an
//! `egui_wgpu` paint callback on the window's own wgpu device, with no copy or readback
//! (ADR-0003). Dragging looks around; the wheel moves forward and back.
//!
//! The renderer is kept in egui-wgpu's callback resources; [`install`] puts it there for a
//! mesh, on the device and formats of the window's render state.

use eframe::egui::{Sense, Ui};
use eframe::egui_wgpu::{self, RenderState, wgpu};
use qs_render::{Camera, MapCallback, MapMesh, MapRenderer, View};

/// The depth buffer the window draws with, which the map view's depth test needs:
/// `depth_buffer: 32` in the app's `NativeOptions`.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Degrees turned per point the pointer drags.
const TURN: f32 = 0.25;
/// Map units moved per point the wheel scrolls.
const TRAVEL: f32 = 2.0;

/// The map shown in the map view and where it is looked at from.
pub struct MapView {
    camera: Camera,
    started: f64,
}

impl MapView {
    /// The view from `camera`.
    #[must_use]
    pub const fn new(camera: Camera) -> Self {
        Self {
            camera,
            started: f64::NAN,
        }
    }

    /// The camera.
    #[must_use]
    pub const fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Draws the map in all the space `ui` has left, after this frame's drag and scroll.
    pub fn ui(&mut self, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::drag());
        let drag = response.drag_delta();
        self.camera.turn(-drag.x * TURN, drag.y * TURN);
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.camera.travel(scroll * TRAVEL, 0.0, 0.0);
        }
        let now = ui.input(|i| i.time);
        if self.started.is_nan() {
            self.started = now;
        }
        let aspect = rect.width() / rect.height().max(1.0);
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            MapCallback {
                view: View {
                    clip_from_world: self.camera.clip_from_world(aspect),
                    eye: self.camera.origin,
                    time: seconds(now - self.started),
                },
                models: qs_render::ModelFrame::default(),
                placements: Vec::new(),
                lightmap: None,
            },
        ));
    }
}

/// Puts a renderer of `mesh` in `state`'s callback resources, for the window's target format
/// and [`DEPTH_FORMAT`], replacing the one there.
pub fn install(state: &RenderState, mesh: &MapMesh) {
    let renderer = MapRenderer::new(
        &state.device,
        &state.queue,
        (state.target_format, DEPTH_FORMAT),
        mesh,
        &[],
    );
    state.renderer.write().callback_resources.insert(renderer);
}

/// Seconds as the shader takes them.
#[expect(
    clippy::cast_possible_truncation,
    reason = "seconds since the view opened, far inside f32's range"
)]
fn seconds(value: f64) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;
