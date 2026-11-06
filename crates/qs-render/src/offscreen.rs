// SPDX-License-Identifier: GPL-3.0-or-later

//! Drawing with no window (P3-05): a wgpu device with no surface, and egui's frames drawn into a
//! texture with a depth buffer, as a window with `depth_buffer: 32` draws them, then read back
//! as RGBA. The headless preview host's `screenshot` and the GPU tests draw with it.

use egui_wgpu::{RenderState, RendererOptions, ScreenDescriptor, wgpu};
use epaint::ClippedPrimitive;
use epaint::textures::TexturesDelta;

/// Why a frame could not be drawn offscreen.
#[derive(Debug)]
pub enum OffscreenError {
    /// No adapter or device: a machine with no GPU and no software renderer.
    Device(String),
    /// The GPU did not finish the frame.
    Poll(String),
    /// The drawn frame could not be read back.
    ReadBack(String),
}

impl std::fmt::Display for OffscreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Device(why) => write!(f, "no GPU device to draw with: {why}"),
            Self::Poll(why) => write!(f, "the GPU did not finish the frame: {why}"),
            Self::ReadBack(why) => write!(f, "the frame could not be read back: {why}"),
        }
    }
}

impl std::error::Error for OffscreenError {}

/// A drawn frame: its size in pixels and its pixels, RGBA, row by row from the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixels {
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
    /// Four bytes a pixel.
    pub rgba: Vec<u8>,
}

/// A wgpu device with no window, and egui's renderer on it.
pub struct Offscreen {
    state: RenderState,
    depth: wgpu::TextureFormat,
    /// The colour and depth targets of the last size drawn, kept for the next frame of that
    /// size, as a swapchain keeps its images.
    targets: Option<Targets>,
}

/// One size's colour and depth targets, and their views.
struct Targets {
    size: [u32; 2],
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
}

impl std::fmt::Debug for Offscreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Offscreen")
            .field("target", &self.state.target_format)
            .field("depth", &self.depth)
            .finish_non_exhaustive()
    }
}

impl Offscreen {
    /// A device with no surface, the first adapter wgpu offers, a target of `Rgba8Unorm` (what
    /// egui's setup picks with no surface) and a depth buffer of `depth`; egui draws with
    /// [`RendererOptions::PREDICTABLE`], no dithering, so a frame is the same each time.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Device`] when there is no adapter or device.
    pub fn new(depth: wgpu::TextureFormat) -> Result<Self, OffscreenError> {
        let setup = egui_wgpu::WgpuSetup::CreateNew(
            egui_wgpu::WgpuSetupCreateNew::without_display_handle(),
        );
        let instance = pollster::block_on(setup.new_instance());
        let configuration = egui_wgpu::WgpuConfiguration {
            wgpu_setup: setup,
            ..egui_wgpu::WgpuConfiguration::default()
        };
        let options = RendererOptions {
            depth_stencil_format: Some(depth),
            ..RendererOptions::PREDICTABLE
        };
        let state = pollster::block_on(RenderState::create(
            &configuration,
            &instance,
            None,
            options,
        ))
        .map_err(|error| OffscreenError::Device(error.to_string()))?;
        Ok(Self {
            state,
            depth,
            targets: None,
        })
    }

    /// Draws with a render state made elsewhere, whose egui renderer was made with a depth
    /// buffer of `depth`.
    #[must_use]
    pub const fn from_state(state: RenderState, depth: wgpu::TextureFormat) -> Self {
        Self {
            state,
            depth,
            targets: None,
        }
    }

    /// The device, its queue, the target format and egui's renderer, whose callback resources
    /// a paint callback's renderer goes in.
    #[must_use]
    pub const fn state(&self) -> &RenderState {
        &self.state
    }

    /// Takes egui's texture changes: each set or freed texture.
    pub fn update_textures(&mut self, delta: &mut TexturesDelta) {
        let mut renderer = self.state.renderer.write();
        let (device, queue) = (&self.state.device, &self.state.queue);
        // Each texture is set or freed on its own, so the map's order does not matter.
        #[expect(clippy::iter_over_hash_type, reason = "each texture is set on its own")]
        for (id, images) in delta.set.drain() {
            for image in images {
                renderer.update_texture(device, queue, id, &image);
            }
        }
        #[expect(
            clippy::iter_over_hash_type,
            reason = "each texture is freed on its own"
        )]
        for id in delta.free.drain() {
            renderer.free_texture(&id);
        }
    }

    /// Draws `primitives`, egui's tessellated frame with its paint callbacks, into a texture of
    /// `screen`'s size cleared to transparent, with a depth buffer cleared to 1, and reads it
    /// back.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Poll`] or [`OffscreenError::ReadBack`] when the GPU fails the frame.
    pub fn render(
        &mut self,
        primitives: &[ClippedPrimitive],
        screen: &ScreenDescriptor,
    ) -> Result<Pixels, OffscreenError> {
        let [width, height] = screen.size_in_pixels;
        let target = self.draw(primitives, screen);
        self.read_back(&target, width, height)
    }

    /// Draws `primitives` as [`Offscreen::render`] does and submits the work, not waiting for
    /// the GPU: a run of frames drawn back to back, finished by [`Offscreen::finish`].
    pub fn submit(&mut self, primitives: &[ClippedPrimitive], screen: &ScreenDescriptor) {
        drop(self.draw(primitives, screen));
    }

    /// Waits for the GPU to finish every frame submitted, as `glFinish ()` does.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Poll`] when the GPU does not finish.
    pub fn finish(&self) -> Result<(), OffscreenError> {
        self.state
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(drop)
            .map_err(|error| OffscreenError::Poll(error.to_string()))
    }

    /// The targets for a `width` by `height` frame, the colour texture and the two views: the
    /// last ones when they are that size, else new ones.
    fn targets(
        &mut self,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView, wgpu::TextureView) {
        let targets = match self.targets.take() {
            Some(targets) if targets.size == [width, height] => targets,
            _ => {
                let state = &self.state;
                let extent = wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                };
                let texture = |format, usage| {
                    state.device.create_texture(&wgpu::TextureDescriptor {
                        label: None,
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                };
                let target = texture(
                    state.target_format,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                );
                let depth = texture(self.depth, wgpu::TextureUsages::RENDER_ATTACHMENT);
                Targets {
                    size: [width, height],
                    target_view: target.create_view(&Default::default()),
                    depth_view: depth.create_view(&Default::default()),
                    target,
                }
            }
        };
        let handles = (
            targets.target.clone(),
            targets.target_view.clone(),
            targets.depth_view.clone(),
        );
        self.targets = Some(targets);
        handles
    }

    /// Draws `primitives` into a new target of `screen`'s size and submits the work.
    fn draw(
        &mut self,
        primitives: &[ClippedPrimitive],
        screen: &ScreenDescriptor,
    ) -> wgpu::Texture {
        let [width, height] = screen.size_in_pixels;
        let (target, target_view, depth_view) = self.targets(width, height);
        let state = &self.state;
        let mut encoder = state.device.create_command_encoder(&Default::default());
        let mut renderer = state.renderer.write();
        let user = renderer.update_buffers(
            &state.device,
            &state.queue,
            &mut encoder,
            primitives,
            screen,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                })
                .forget_lifetime();
            renderer.render(&mut pass, primitives, screen);
        }
        drop(renderer);
        state
            .queue
            .submit(user.into_iter().chain([encoder.finish()]));
        target
    }

    /// `texture`'s pixels, its rows copied out of wgpu's padded rows.
    fn read_back(
        &self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<Pixels, OffscreenError> {
        let state = &self.state;
        let size_error = || OffscreenError::ReadBack(format!("a frame of {width} by {height}"));
        let unpadded = width.checked_mul(4).ok_or_else(size_error)?;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded
            .div_ceil(align)
            .checked_mul(align)
            .ok_or_else(size_error)?;
        let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(padded).saturating_mul(u64::from(height)),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = state.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
        state.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        state
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| OffscreenError::Poll(error.to_string()))?;
        let data = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|error| OffscreenError::ReadBack(error.to_string()))?;
        let row = usize::try_from(padded).map_err(|_| size_error())?;
        let keep = usize::try_from(unpadded).map_err(|_| size_error())?;
        let mut rgba: Vec<u8> = data
            .chunks(row)
            .flat_map(|line| line.iter().take(keep).copied())
            .collect();
        to_rgba(state.target_format, &mut rgba);
        Ok(Pixels {
            width,
            height,
            rgba,
        })
    }
}

/// Puts the bytes of pixels of `format` in RGBA order: a window's surface on macOS is
/// `Bgra8Unorm`, whose red and blue swap places; RGBA formats stay as they are.
fn to_rgba(format: wgpu::TextureFormat, bytes: &mut [u8]) {
    if matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for pixel in bytes.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "test code: a failed unwrap or index is a failed test; frames are a few pixels"
    )]

    use egui_wgpu::{ScreenDescriptor, wgpu};
    use epaint::textures::{TextureOptions, TexturesDelta};
    use epaint::{
        ClippedPrimitive, Color32, ColorImage, ImageDelta, Mesh, Pos2, Primitive, Rect, TextureId,
    };

    use super::{Offscreen, OffscreenError, to_rgba};

    /// A screen of `width` by `height` pixels at one pixel a point.
    const fn screen(width: u32, height: u32) -> ScreenDescriptor {
        ScreenDescriptor {
            pixels_per_point: 1.0,
            size_in_pixels: [width, height],
        }
    }

    /// The pixel at (`x`, `y`) of a frame `width` wide.
    fn at(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = usize::try_from((y * width + x) * 4).unwrap();
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    // P3-05: a frame drawn with no window is the size asked, its rows read back without wgpu's
    // padding (a row of 3 pixels is padded to 256 bytes), egui's mesh drawn where it lies and
    // the rest the transparent clear
    #[test]
    fn a_frame_is_drawn_and_read_back_with_no_window() {
        let mut offscreen = Offscreen::new(wgpu::TextureFormat::Depth32Float).unwrap();
        let mut white = TexturesDelta::default();
        white
            .set
            .entry(TextureId::Managed(0))
            .or_default()
            .push(ImageDelta::full(
                ColorImage::new([1, 1], vec![Color32::WHITE]),
                TextureOptions::NEAREST,
            ));
        offscreen.update_textures(&mut white);
        assert!(white.set.is_empty(), "the delta is taken");
        let (width, height) = (3, 2);
        let mut mesh = Mesh::with_texture(TextureId::Managed(0));
        mesh.add_rect_with_uv(
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 2.0)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::RED,
        );
        let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(3.0, 2.0));
        let primitives = [ClippedPrimitive {
            clip_rect: full,
            primitive: Primitive::Mesh(mesh),
        }];
        let pixels = offscreen
            .render(&primitives, &screen(width, height))
            .unwrap();
        assert_eq!((pixels.width, pixels.height), (width, height));
        assert_eq!(pixels.rgba.len(), 3 * 2 * 4, "no padding left in");
        for y in 0..height {
            assert_eq!(at(&pixels.rgba, width, 0, y), [255, 0, 0, 255], "row {y}");
            assert_eq!(at(&pixels.rgba, width, 2, y), [0, 0, 0, 0], "row {y}");
        }
        let empty = offscreen.render(&[], &screen(5, 1)).unwrap();
        assert!(empty.rgba.iter().all(|&byte| byte == 0));
        assert_eq!(empty.rgba.len(), 5 * 4);
        let mut free = TexturesDelta::default();
        free.free.insert(TextureId::Managed(0));
        offscreen.update_textures(&mut free);
        assert!(free.free.is_empty());
        assert!(format!("{offscreen:?}").contains("Depth32Float"));
    }

    // P3-05: a BGRA target's pixels are read back in RGBA order; an RGBA target's are as they are
    #[test]
    fn pixels_come_back_in_rgba_order() {
        for format in [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ] {
            let mut bytes = [1, 2, 3, 4, 5, 6, 7, 8];
            to_rgba(format, &mut bytes);
            assert_eq!(bytes, [3, 2, 1, 4, 7, 6, 5, 8], "{format:?}");
        }
        let mut bytes = [1, 2, 3, 4];
        to_rgba(wgpu::TextureFormat::Rgba8Unorm, &mut bytes);
        assert_eq!(bytes, [1, 2, 3, 4]);
    }

    // No spec statement: each failure says what failed
    #[test]
    fn errors_say_what_failed() {
        let cases = [
            (OffscreenError::Device("x".to_owned()), "no GPU device"),
            (OffscreenError::Poll("x".to_owned()), "did not finish"),
            (OffscreenError::ReadBack("x".to_owned()), "read back"),
        ];
        for (error, words) in cases {
            assert!(error.to_string().contains(words), "{error}");
            assert!(error.to_string().ends_with(": x"), "{error}");
        }
    }
}
