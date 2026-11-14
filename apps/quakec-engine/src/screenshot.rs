// SPDX-License-Identifier: GPL-3.0-or-later

//! `screenshot`'s image (BRIDGE-4.9): the frame drawn offscreen, by the window's device or the
//! headless host's, scaled to the size asked, as a PNG in base64.

use eframe::egui;
use eframe::egui_wgpu::{self, ScreenDescriptor};
use image::codecs::png::PngEncoder;
use image::imageops::{FilterType, resize};
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use qs_bridge::base64;
use qs_render::{MapCallback, Offscreen};

/// Draws a frame's paint callback offscreen for `screenshot`: on a device with no window in
/// the headless host, and on the window's own in the windowed one, whose renderer is in that
/// device's callback resources. The window's view is drawn again, not captured: eframe's
/// capture of the window's surface never answered on macOS, as its read-back waits for a device
/// poll nothing makes.
pub(crate) struct Drawer {
    offscreen: Offscreen,
    ctx: egui::Context,
}

impl Drawer {
    /// A drawer on `offscreen`'s device.
    pub(crate) fn new(offscreen: Offscreen) -> Self {
        Self {
            offscreen,
            ctx: egui::Context::default(),
        }
    }

    /// The device, to give the map's renderer to.
    pub(crate) const fn offscreen(&self) -> &Offscreen {
        &self.offscreen
    }

    /// `callback`, the view of one frame, drawn at `size` pixels and read back.
    pub(crate) fn draw(&mut self, callback: MapCallback, size: [u32; 2]) -> Result<Frame, String> {
        let (primitives, screen) = self.layout(callback, size);
        let pixels = self
            .offscreen
            .render(&primitives, &screen)
            .map_err(|error| error.to_string())?;
        Ok(Frame {
            width: pixels.width,
            height: pixels.height,
            rgba: pixels.rgba,
        })
    }

    /// `callback`, the view of one frame, laid out, drawn at `size` pixels and submitted, not
    /// waited for; [`Drawer::finish`] waits for every frame submitted.
    pub(crate) fn submit(&mut self, callback: MapCallback, size: [u32; 2]) {
        let (primitives, screen) = self.layout(callback, size);
        self.offscreen.submit(&primitives, &screen);
    }

    /// Waits for the GPU to finish the frames submitted.
    pub(crate) fn finish(&self) -> Result<(), String> {
        self.offscreen.finish().map_err(|error| error.to_string())
    }

    /// `callback` laid out over a screen of `size` pixels: egui's primitives and the screen.
    fn layout(
        &mut self,
        callback: MapCallback,
        size: [u32; 2],
    ) -> (Vec<egui::ClippedPrimitive>, ScreenDescriptor) {
        let [width, height] = size.map(|n| n.max(1));
        let points = |n: u32| f32::from(u16::try_from(n).unwrap_or(u16::MAX));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(points(width), points(height)),
            )),
            ..egui::RawInput::default()
        };
        let mut callback = Some(callback);
        let mut output = self.ctx.run_ui(input, |ui| {
            if let Some(callback) = callback.take() {
                let rect = ui.max_rect();
                ui.painter()
                    .add(egui_wgpu::Callback::new_paint_callback(rect, callback));
            }
        });
        self.offscreen.update_textures(&mut output.textures_delta);
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = ScreenDescriptor {
            pixels_per_point: output.pixels_per_point,
            size_in_pixels: [width, height],
        };
        (primitives, screen)
    }
}

/// A frame of `width` by `height` pixels, RGBA row by row from the top.
pub(crate) struct Frame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

impl Frame {
    /// `frame` as `screenshot`'s result: scaled to `size` when one is asked, encoded as a PNG in
    /// base64, with its size.
    ///
    /// # Errors
    ///
    /// Why the frame did not encode.
    pub(crate) fn encode(self, size: Option<(u32, u32)>) -> Result<(String, u32, u32), String> {
        let image = RgbaImage::from_raw(self.width, self.height, self.rgba)
            .ok_or("the frame is not its size")?;
        let image = match size {
            Some((width, height)) if (width, height) != image.dimensions() => {
                resize(&image, width, height, FilterType::Triangle)
            }
            _ => image,
        };
        let (width, height) = image.dimensions();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
            .map_err(|e| e.to_string())?;
        Ok((base64(&png), width, height))
    }
}

#[cfg(test)]
mod tests;
