// SPDX-License-Identifier: GPL-3.0-or-later

//! A [`MapMesh`] on the GPU, drawn inside egui's render pass through a paint callback
//! (ADR-0003): the map view and the preview host share it. The frame is drawn in GLQuake's
//! order (`R_RenderView`): the world, the alias models and brush entities, the sprites, the
//! particles, the view model, the water, the view blend and the 2D layer.

use egui_wgpu::wgpu::util::{BufferInitDescriptor, DeviceExt, TextureDataOrder};
use egui_wgpu::wgpu::{
    self, BindGroup, BindGroupLayout, Buffer, Device, Queue, RenderPipeline, Sampler, TextureFormat,
};
use std::sync::Arc;

use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};

use crate::alias::AliasVertex;
use crate::mesh::{Animation, Batch, Image, MapMesh, Placement, Surface, frame, mip_levels};
use crate::particles::{ParticleVertex, particle_texture};
use crate::skybox::Skybox;
use crate::sprite::SpriteVertex;

/// The bytes of a [`crate::mesh::Vertex`] on the GPU: position, texture and light coordinates.
const VERTEX_BYTES: u64 = 7 * 4;
/// The bytes of an [`AliasVertex`] on the GPU: position, skin coordinates and shade.
const ALIAS_VERTEX_BYTES: u64 = 6 * 4;
/// The bytes of a [`SpriteVertex`]: position and texture coordinates.
const SPRITE_VERTEX_BYTES: u64 = 5 * 4;
/// The bytes of a [`ParticleVertex`]: position, texture coordinates and colour.
const PARTICLE_VERTEX_BYTES: u64 = 9 * 4;
/// The bytes of a [`FlatVertex`]: position, texture coordinates and colour.
const FLAT_VERTEX_BYTES: u64 = 8 * 4;
/// The bytes of the view's uniform: a 4 by 4 matrix, the eye's position and the time, the
/// fog, the parameters and the skybox's.
const VIEW_BYTES: u64 = 32 * 4;
/// A skybox's faces, one texture layer each (REND-8.2).
const SKY_FACES: u32 = 6;
/// `glDepthRange (gldepthmin, gldepthmin + 0.3*(gldepthmax-gldepthmin))`: the view model is
/// drawn in the nearest 0.3 of the depth range, so it never pokes into walls (REND-4.11).
const VIEW_MODEL_DEPTH: f32 = 0.3;
/// The whole depth range.
const FULL_DEPTH: (f32, f32) = (0.0, 1.0);
/// `GL_PolygonOffset (OFFSET_DECAL)`, `glPolygonOffset (-1, -1)`: an oriented sprite is
/// pulled toward the eye so it sits on the wall it is placed on (REND-11.2).
const DECAL_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState {
    constant: -1,
    slope_scale: -1.0,
    clamp: 0.0,
};

/// A map's buffers, textures and pipeline, kept in egui's callback resources.
pub struct MapRenderer {
    /// The pipelines of lit, turbulent and sky surfaces.
    pipelines: [RenderPipeline; 3],
    vertices: Buffer,
    indices: Buffer,
    view: Buffer,
    shared: BindGroup,
    textures: Vec<BindGroup>,
    batches: Vec<Batch>,
    animations: Vec<Option<Animation>>,
    /// The pipeline of alias models (REND-4).
    alias_pipeline: RenderPipeline,
    /// Each alias skin image, bound as a surface texture, then [`PLAYER_SKINS`] player skins,
    /// each a client slot's skin in its colours (REND-4.13).
    skins: Vec<BindGroup>,
    /// The layout and the sampler a skin is bound with, for a player skin set later.
    skin_binding: (BindGroupLayout, Sampler),
    /// What each player skin slot holds, `None` while it is the white placeholder.
    player_keys: [Option<SkinKey>; PLAYER_SKINS],
    /// This frame's alias triangles, and the view model's.
    alias: Grown,
    alias_draws: Vec<AliasDraw>,
    weapon: Grown,
    weapon_draws: Vec<AliasDraw>,
    /// The pipelines of sprites: plain, and pulled toward the eye for decals (REND-11.2).
    sprite_pipelines: [RenderPipeline; 2],
    /// Each sprite frame image.
    frames: Vec<BindGroup>,
    sprites: Grown,
    sprite_draws: Vec<SpriteDraw>,
    /// The pipeline of particles and the particle texture (PART-3.2).
    particle_pipeline: RenderPipeline,
    particle_texture: BindGroup,
    particles: Grown,
    particle_count: u32,
    /// The 2D layer's pipelines: pictures with the alpha test, and blends (HUD-1, VIEW-4.6).
    flat_pipelines: [RenderPipeline; 2],
    /// Each picture, and the white texel for fills and blends.
    pics: Vec<BindGroup>,
    white: BindGroup,
    flat: Grown,
    flat_draws: Vec<FlatDraw>,
    /// Each model's matrix, one every [`MODEL_STRIDE`] bytes, the world's first (REND-1.9).
    models: Buffer,
    model_group: BindGroup,
    model_count: usize,
    /// Whether each model is drawn this frame: a brush entity not in the visible list is not.
    shown: Vec<bool>,
    /// The lightmap atlas, written again as the light styles change (REND-5.5).
    lightmap: wgpu::Texture,
    /// The skybox's face size, `None` for the scrolling layers (REND-8.2).
    skybox: Option<[u32; 2]>,
}

/// A vertex buffer that grows to what a frame needs.
struct Grown {
    label: &'static str,
    buffer: Option<(Buffer, u64)>,
    /// The bytes of one vertex, the least a buffer holds.
    vertex: u64,
}

impl Grown {
    const fn new(label: &'static str, vertex: u64) -> Self {
        Self {
            label,
            buffer: None,
            vertex,
        }
    }

    /// Writes `bytes`, making a larger buffer first when the one there is too small.
    fn write(&mut self, device: &Device, queue: &Queue, bytes: &[u8]) {
        let needed = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if let Some(size) = grown(
            self.buffer.as_ref().map(|(_, size)| *size),
            needed,
            self.vertex,
        ) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.buffer = Some((buffer, size));
        }
        if let Some((buffer, _)) = &self.buffer {
            queue.write_buffer(buffer, 0, bytes);
        }
    }

    fn slice(&self) -> Option<wgpu::BufferSlice<'_>> {
        self.buffer.as_ref().map(|(buffer, _)| buffer.slice(..))
    }
}

/// The bytes between two models' matrices: the dynamic offset alignment every device allows.
const MODEL_STRIDE: u64 = 256;
/// The bytes of one model's matrix.
const MATRIX_BYTES: u64 = 16 * 4;

/// The player skins the renderer keeps after the models' skins, one per client slot,
/// GLQuake's `playertextures` for `MAX_SCOREBOARD` 16 clients (REND-4.13).
pub(crate) const PLAYER_SKINS: usize = 16;

/// One run of a frame's alias vertices, drawn with one skin image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AliasDraw {
    /// The index of its image among the skins the renderer was made with.
    pub skin: usize,
    /// Its first vertex.
    pub first: u32,
    /// Its vertices, three a triangle.
    pub count: u32,
}

/// What a player slot's translated skin was made from (REND-4.13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkinKey {
    /// The model's precache index.
    pub model: usize,
    /// The skin.
    pub skin: usize,
    /// The slot's colours, shirt high and trousers low.
    pub colors: i32,
}

/// A player slot's translated skin, sent with every frame that draws the player; the
/// renderer uploads it when its key is not the one the slot holds (REND-4.13).
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerSkin {
    /// The client slot, 0 to 15.
    pub slot: usize,
    /// What the image was made from.
    pub key: SkinKey,
    /// The skin in the slot's colours.
    pub image: Arc<Image>,
}

/// The alias models of one frame: their vertices and the runs drawn with each skin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelFrame {
    /// Every model's vertices, three a triangle.
    pub vertices: Vec<AliasVertex>,
    /// The runs of `vertices`.
    pub draws: Vec<AliasDraw>,
}

/// One sprite's quad, drawn with one frame image (REND-11.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteDraw {
    /// The index of its image among the sprite frames the renderer was made with.
    pub image: usize,
    /// Its first vertex.
    pub first: u32,
    /// Its vertices, six.
    pub count: u32,
    /// Whether it is a decal on a wall, drawn with the polygon offset.
    pub oriented: bool,
}

/// The sprites of one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpriteFrame {
    /// Every sprite's vertices, three a triangle.
    pub vertices: Vec<SpriteVertex>,
    /// The runs of `vertices`.
    pub draws: Vec<SpriteDraw>,
}

/// One corner of the 2D layer's quads: a picture, a character, a fill or a blend (HUD-1 to
/// HUD-3, VIEW-4.6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatVertex {
    /// Its position in the layer's units, from the top left (`glOrtho`).
    pub position: [f32; 2],
    /// Its texture coordinates.
    pub texture: [f32; 2],
    /// Its colour, which the texel is multiplied by.
    pub color: [f32; 4],
}

/// One run of the 2D layer's vertices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatDraw {
    /// The picture it is drawn with, an index among the pictures the renderer was made with;
    /// `None` for the white texel, a fill or a blend.
    pub pic: Option<usize>,
    /// Whether it is blended over the screen with no alpha test (the view blend,
    /// `Draw_FadeScreen`) instead of alpha tested (a picture, a character, a fill).
    pub blend: bool,
    /// Its first vertex.
    pub first: u32,
    /// Its vertices, six a quad.
    pub count: u32,
}

/// The 2D layer of one frame, in drawing order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatFrame {
    /// Every quad's vertices, three a triangle.
    pub vertices: Vec<FlatVertex>,
    /// The runs of `vertices`.
    pub draws: Vec<FlatDraw>,
}

/// The images the renderer is made with besides the mesh's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Assets<'a> {
    /// Every alias model's skin images (REND-4.3).
    pub skins: &'a [Image],
    /// Every sprite's frame images (REND-11.2).
    pub frames: &'a [Image],
    /// The 2D pictures: `gfx.wad`'s, the `gfx/*.lmp` files and the console characters
    /// (HUD-1 to HUD-3).
    pub pics: &'a [Image],
    /// The map's skybox, drawn where the sky faces are; `None` for the scrolling layers
    /// (REND-8.2).
    pub skybox: Option<&'a Skybox>,
}

/// The pipeline of each kind of surface, an index into [`MapRenderer`]'s pipelines.
const fn pipeline_of(surface: Surface) -> usize {
    match surface {
        Surface::Lit => 0,
        Surface::Turbulent => 1,
        Surface::Sky => 2,
    }
}

/// How a pipeline writes its fragments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pass {
    /// Opaque, writing depth.
    Opaque,
    /// `glBlendFunc (GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA)`, writing depth: the water.
    Blended,
    /// Blended, with `glDepthMask (GL_FALSE)`: the particles.
    BlendedNoDepthWrite,
    /// Blended, with no depth test: the 2D layer.
    Flat,
    /// Opaque, pulled toward the eye: a decal.
    Decal,
}

impl MapRenderer {
    /// Uploads `mesh` and `assets`, and builds the pipelines for a target of `format` with a
    /// depth buffer of `depth` (the format egui's render pass was made with).
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "one pipeline's layouts, buffers and bind groups read best in the order wgpu needs them"
    )]
    pub fn new(
        device: &Device,
        queue: &Queue,
        (format, depth): (TextureFormat, TextureFormat),
        mesh: &MapMesh,
        assets: &Assets<'_>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("qs-render world"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let shared_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("qs-render view and lightmap"),
            entries: &[
                // The matrix is read by the vertex stage, the eye and the time by the water's and
                // the sky's fragments (REND-2.2, REND-2.3).
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                sampler_entry(2),
                // The skybox's six faces (REND-8.2).
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("qs-render surface texture"),
            // A surface's texture and its sampler; for the sky, its solid layer, and its alpha
            // layer at 2 (REND-2.3).
            entries: &[texture_entry(0), sampler_entry(1), texture_entry(2)],
        });
        let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("qs-render model"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(MATRIX_BYTES),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("qs-render world"),
            bind_group_layouts: &[
                Some(&shared_layout),
                Some(&texture_layout),
                Some(&model_layout),
            ],
            immediate_size: 0,
        });
        let model_count = mesh.models.len().max(1);
        let models = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("qs-render models"),
            size: MODEL_STRIDE.saturating_mul(u64::try_from(model_count).unwrap_or(1)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let model_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("qs-render model"),
            layout: &model_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &models,
                    offset: 0,
                    size: wgpu::BufferSize::new(MATRIX_BYTES),
                }),
            }],
        });
        let placed: Vec<[[f32; 4]; 4]> = mesh.models.iter().map(Placement::matrix).collect();
        queue.write_buffer(&models, 0, &model_bytes(&placed));
        let world_vertex = wgpu::VertexBufferLayout {
            array_stride: VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2],
        };
        let alias_vertex = wgpu::VertexBufferLayout {
            array_stride: ALIAS_VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32],
        };
        let sprite_vertex = wgpu::VertexBufferLayout {
            array_stride: SPRITE_VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
        };
        let particle_vertex = wgpu::VertexBufferLayout {
            array_stride: PARTICLE_VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4],
        };
        let flat_vertex = wgpu::VertexBufferLayout {
            array_stride: FLAT_VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
        };
        let pipeline = |(vertex, layout_of): (&str, &wgpu::VertexBufferLayout<'_>),
                        entry: &str,
                        pass: Pass| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("qs-render world"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(layout_of.clone())],
                },
                primitive: primitive(pass),
                depth_stencil: Some(depth_stencil(depth, pass)),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: blend_of(pass),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let world = ("vertex", &world_vertex);
        let pipelines = [
            pipeline(world, "fragment", Pass::Opaque),
            pipeline(world, "turbulent", Pass::Blended),
            pipeline(world, "sky", Pass::Opaque),
        ];
        let alias_pipeline = pipeline(
            ("alias_vertex", &alias_vertex),
            "alias_fragment",
            Pass::Opaque,
        );
        let sprite = ("sprite_vertex", &sprite_vertex);
        let sprite_pipelines = [
            pipeline(sprite, "sprite_fragment", Pass::Opaque),
            pipeline(sprite, "sprite_fragment", Pass::Decal),
        ];
        let particle_pipeline = pipeline(
            ("particle_vertex", &particle_vertex),
            "particle_fragment",
            Pass::BlendedNoDepthWrite,
        );
        let flat = ("flat_vertex", &flat_vertex);
        let flat_pipelines = [
            pipeline(flat, "flat_fragment", Pass::Flat),
            pipeline(flat, "blend_fragment", Pass::Flat),
        ];
        let vertex_bytes: Vec<u8> = mesh
            .vertices
            .iter()
            .flat_map(|v| {
                [
                    v.position[0],
                    v.position[1],
                    v.position[2],
                    v.texture[0],
                    v.texture[1],
                    v.light[0],
                    v.light[1],
                ]
            })
            .flat_map(f32::to_ne_bytes)
            .collect();
        let index_bytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_ne_bytes()).collect();
        let vertices = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("qs-render vertices"),
            contents: &vertex_bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("qs-render indices"),
            contents: &index_bytes,
            usage: wgpu::BufferUsages::INDEX,
        });
        let view = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("qs-render view"),
            size: VIEW_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Textures keep Quake's sharp texels; the lightmap is smoothed, as GLQuake draws it.
        let mipmapped = device.create_sampler(&world_sampler());
        let linear_repeat = sampler(device, wgpu::FilterMode::Linear, wgpu::AddressMode::Repeat);
        let linear = sampler(
            device,
            wgpu::FilterMode::Linear,
            wgpu::AddressMode::ClampToEdge,
        );
        let lightmap_texture = upload_texture(
            device,
            queue,
            core::slice::from_ref(&mesh.lightmap),
            // COPY_DST to write it again as the styles change; COPY_SRC so it can be read
            // back, as the tests do.
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
        );
        let lightmap = lightmap_texture.create_view(&wgpu::TextureViewDescriptor::default());
        // REND-8.2: a box whose faces the device cannot hold keeps the scrolling layers.
        let limit = device.limits().max_texture_dimension_2d;
        let fitting = assets
            .skybox
            .filter(|skybox| skybox.size().iter().all(|&side| side <= limit));
        let skybox = upload_skybox(device, queue, fitting);
        let shared = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("qs-render view and lightmap"),
            layout: &shared_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&lightmap),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&linear),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&skybox),
                },
            ],
        });
        let bind =
            |texture: &wgpu::TextureView, filter: &wgpu::Sampler, second: &wgpu::TextureView| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("qs-render surface texture"),
                    layout: &texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(texture),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(filter),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(second),
                        },
                    ],
                })
            };
        // REND-4.3: a skin is mipmapped and filtered as a world texture is.
        let mut skins: Vec<BindGroup> = assets
            .skins
            .iter()
            .map(|image| {
                let texture = upload_levels(device, queue, &mip_levels(image));
                bind(&texture, &mipmapped, &texture)
            })
            .collect();
        // REND-4.13: each player skin starts white, until its slot's colours set it.
        let blank = Image {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        };
        for _ in 0..PLAYER_SKINS {
            let texture = upload_levels(device, queue, &mip_levels(&blank));
            skins.push(bind(&texture, &mipmapped, &texture));
        }
        // A sprite frame and a picture keep their transparent texels: no mipmaps, filtered
        // linearly as `GL_LoadTexture (..., alpha)` filters them.
        let plain = |image: &Image| {
            let texture = upload(device, queue, image);
            bind(&texture, &linear, &texture)
        };
        let frames = assets.frames.iter().map(plain).collect();
        let pics = assets.pics.iter().map(plain).collect();
        let particle_texture = plain(&particle_texture());
        let white = plain(&Image {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        });
        let textures = mesh
            .textures
            .iter()
            .enumerate()
            .map(|(slot, image)| {
                let sky = mesh.skies.get(slot).and_then(Option::as_ref);
                // A sky's two layers, each repeating on its own, filtered linearly as GLQuake
                // filters them (`R_InitSky`); any other texture is its own second binding.
                let (texture, second, filter) = match sky {
                    Some([solid, alpha]) => (
                        upload(device, queue, solid),
                        upload(device, queue, alpha),
                        &linear_repeat,
                    ),
                    None => {
                        let texture = upload_levels(device, queue, &mip_levels(image));
                        (texture.clone(), texture, &mipmapped)
                    }
                };
                bind(&texture, filter, &second)
            })
            .collect();
        Self {
            pipelines,
            vertices,
            indices,
            view,
            shared,
            textures,
            batches: mesh.batches.clone(),
            animations: mesh.animations.clone(),
            alias_pipeline,
            skins,
            skin_binding: (texture_layout.clone(), mipmapped.clone()),
            player_keys: [None; PLAYER_SKINS],
            alias: Grown::new("qs-render alias vertices", ALIAS_VERTEX_BYTES),
            alias_draws: Vec::new(),
            weapon: Grown::new("qs-render view model vertices", ALIAS_VERTEX_BYTES),
            weapon_draws: Vec::new(),
            sprite_pipelines,
            frames,
            sprites: Grown::new("qs-render sprite vertices", SPRITE_VERTEX_BYTES),
            sprite_draws: Vec::new(),
            particle_pipeline,
            particle_texture,
            particles: Grown::new("qs-render particle vertices", PARTICLE_VERTEX_BYTES),
            particle_count: 0,
            flat_pipelines,
            pics,
            white,
            flat: Grown::new("qs-render 2D vertices", FLAT_VERTEX_BYTES),
            flat_draws: Vec::new(),
            models,
            model_group,
            model_count,
            shown: vec![true; model_count],
            lightmap: lightmap_texture,
            skybox: fitting.map(Skybox::size),
        }
    }

    /// Moves the models to `matrices`, the world's first, as the entities stand now
    /// (REND-1.9); a model given `None` is not drawn, and a matrix past the mesh's models is
    /// left out.
    pub fn set_placements(&mut self, queue: &Queue, matrices: &[Option<[[f32; 4]; 4]>]) {
        let take = matrices.len().min(self.model_count);
        let matrices = matrices.get(..take).unwrap_or(&[]);
        for (shown, matrix) in self.shown.iter_mut().zip(matrices) {
            *shown = matrix.is_some();
        }
        let placed: Vec<[[f32; 4]; 4]> = matrices
            .iter()
            .map(|m| m.unwrap_or(Placement::WORLD.matrix()))
            .collect();
        queue.write_buffer(&self.models, 0, &model_bytes(&placed));
    }

    /// Sets the alias models drawn next, growing their buffer when they need more room.
    pub fn set_models(&mut self, device: &Device, queue: &Queue, models: &ModelFrame) {
        self.alias
            .write(device, queue, &alias_bytes(&models.vertices));
        self.alias_draws.clone_from(&models.draws);
    }

    /// Sets player skin `skin.slot`, a client slot's skin in its colours (REND-4.13), to its
    /// image, mipmapped and filtered as any skin is (REND-4.3), unless the slot already holds
    /// its key. The skin a draw names is the models' skins' count plus the slot.
    pub fn set_player_skin(&mut self, device: &Device, queue: &Queue, skin: &PlayerSkin) {
        let Some(held) = self.player_keys.get_mut(skin.slot) else {
            return;
        };
        if *held == Some(skin.key) {
            return;
        }
        *held = Some(skin.key);
        let (layout, sampler) = &self.skin_binding;
        let at = self
            .skins
            .len()
            .saturating_sub(PLAYER_SKINS)
            .saturating_add(skin.slot);
        let texture = upload_levels(device, queue, &mip_levels(&skin.image));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("qs-render player skin"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&texture),
                },
            ],
        });
        if let Some(entry) = self.skins.get_mut(at) {
            *entry = group;
        }
    }

    /// Sets the view model drawn next, in the nearest 0.3 of the depth range (REND-4.11).
    pub fn set_weapon(&mut self, device: &Device, queue: &Queue, weapon: &ModelFrame) {
        self.weapon
            .write(device, queue, &alias_bytes(&weapon.vertices));
        self.weapon_draws.clone_from(&weapon.draws);
    }

    /// Sets the sprites drawn next (REND-11).
    pub fn set_sprites(&mut self, device: &Device, queue: &Queue, sprites: &SpriteFrame) {
        let bytes: Vec<u8> = sprites
            .vertices
            .iter()
            .flat_map(|v| {
                let [x, y, z] = v.position;
                let [s, t] = v.texture;
                [x, y, z, s, t]
            })
            .flat_map(f32::to_ne_bytes)
            .collect();
        self.sprites.write(device, queue, &bytes);
        self.sprite_draws.clone_from(&sprites.draws);
    }

    /// Sets the particles drawn next (PART-3.2).
    pub fn set_particles(&mut self, device: &Device, queue: &Queue, particles: &[ParticleVertex]) {
        let bytes: Vec<u8> = particles
            .iter()
            .flat_map(|v| {
                let [x, y, z] = v.position;
                let [s, t] = v.texture;
                let [r, g, b, a] = v.color;
                [x, y, z, s, t, r, g, b, a]
            })
            .flat_map(f32::to_ne_bytes)
            .collect();
        self.particles.write(device, queue, &bytes);
        self.particle_count = u32::try_from(particles.len()).unwrap_or(u32::MAX);
    }

    /// Sets the 2D layer drawn next (HUD-1 to HUD-3, VIEW-4.6).
    pub fn set_flat(&mut self, device: &Device, queue: &Queue, flat: &FlatFrame) {
        self.flat.write(device, queue, &flat_bytes(&flat.vertices));
        self.flat_draws.clone_from(&flat.draws);
    }

    /// Writes `image`, the mesh's lightmap made again for new light styles
    /// ([`crate::MapMesh::relight`]), over the one drawn (REND-5.5). An image of another size
    /// than the mesh's lightmap is not written.
    pub fn set_lightmap(&self, queue: &Queue, image: &Image) {
        let size = self.lightmap.size();
        if (image.width, image.height) != (size.width, size.height) {
            return;
        }
        queue.write_texture(
            self.lightmap.as_image_copy(),
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width.saturating_mul(4)),
                rows_per_image: Some(image.height),
            },
            size,
        );
    }

    /// Sets the view for the next draw: the matrix from map coordinates to clip space, the
    /// eye's position (REND-2.3), the time in seconds (REND-2.2, REND-2.3), the fog
    /// (REND-10), the water alpha (REND-9), the sky's fog (REND-8) and the 2D layer's size.
    pub fn set_view(&self, queue: &Queue, view: &View) {
        let eye_time = [view.eye[0], view.eye[1], view.eye[2], view.time];
        let [r, g, b] = view.fog.color;
        let fog = [r, g, b, view.fog.density / REND_FOG_DENSITY_SCALE];
        // REND-8.1: the fog's share of the sky only while there is fog.
        let skyfog = if view.fog.density > 0.0 {
            view.skyfog.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let params = [view.water_alpha, skyfog, view.screen[0], view.screen[1]];
        // REND-8.2: whether the sky is the box, and its faces' size.
        let sky = self
            .skybox
            .map_or([0.0; 4], |[w, h]| [1.0, f32_of_u32(w), f32_of_u32(h), 0.0]);
        let bytes: Vec<u8> = view
            .clip_from_world
            .iter()
            .flatten()
            .chain(&eye_time)
            .chain(&fog)
            .chain(&params)
            .chain(&sky)
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        queue.write_buffer(&self.view, 0, &bytes);
    }

    /// Draws the frame into `pass` in `R_RenderView`'s order, each animated texture at its
    /// frame for `time` seconds (REND-2.1); `viewport` is the pass's viewport in pixels,
    /// which the view model's depth range is set on.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, time: f32, viewport: Option<[f32; 4]>) {
        pass.set_bind_group(0, &self.shared, &[]);
        self.draw_world(pass, time, Surface::Lit);
        self.draw_world(pass, time, Surface::Sky);
        self.draw_alias(pass, &self.alias, &self.alias_draws);
        self.draw_sprites(pass);
        self.draw_particles(pass);
        // REND-4.11: the view model in the nearest 0.3 of the depth range.
        if let Some([x, y, w, h]) = viewport {
            pass.set_viewport(x, y, w, h, FULL_DEPTH.0, VIEW_MODEL_DEPTH);
            self.draw_alias(pass, &self.weapon, &self.weapon_draws);
            pass.set_viewport(x, y, w, h, FULL_DEPTH.0, FULL_DEPTH.1);
        }
        // REND-9.1: the water after everything else, blended.
        self.draw_world(pass, time, Surface::Turbulent);
        self.draw_flat(pass);
    }

    /// The batches of `kind` (REND-1.11).
    fn draw_world(&self, pass: &mut wgpu::RenderPass<'_>, time: f32, kind: Surface) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for batch in self.batches.iter().filter(|b| b.surface == kind) {
            if !self.shown.get(batch.model).copied().unwrap_or(true) {
                continue;
            }
            let frame = frame(&self.animations, batch.texture, time);
            if let (Some(texture), Some(pipeline)) = (
                self.textures.get(frame),
                self.pipelines.get(pipeline_of(batch.surface)),
            ) {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(1, texture, &[]);
                pass.set_bind_group(2, &self.model_group, &[model_offset(batch.model)]);
                pass.draw_indexed(
                    batch.first..batch.first.saturating_add(batch.count),
                    0,
                    0..1,
                );
            }
        }
    }

    fn draw_alias(&self, pass: &mut wgpu::RenderPass<'_>, buffer: &Grown, draws: &[AliasDraw]) {
        let Some(slice) = buffer.slice() else {
            return;
        };
        pass.set_pipeline(&self.alias_pipeline);
        pass.set_vertex_buffer(0, slice);
        // Alias vertices are already where they are drawn: the world's matrix.
        pass.set_bind_group(2, &self.model_group, &[0]);
        for draw in draws {
            if let Some(skin) = self.skins.get(draw.skin) {
                pass.set_bind_group(1, skin, &[]);
                pass.draw(draw.first..draw.first.saturating_add(draw.count), 0..1);
            }
        }
    }

    fn draw_sprites(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(slice) = self.sprites.slice() else {
            return;
        };
        pass.set_vertex_buffer(0, slice);
        pass.set_bind_group(2, &self.model_group, &[0]);
        for draw in &self.sprite_draws {
            let pipeline = self.sprite_pipelines.get(usize::from(draw.oriented));
            if let (Some(image), Some(pipeline)) = (self.frames.get(draw.image), pipeline) {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(1, image, &[]);
                pass.draw(draw.first..draw.first.saturating_add(draw.count), 0..1);
            }
        }
    }

    fn draw_particles(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(slice) = self.particles.slice() else {
            return;
        };
        if self.particle_count == 0 {
            return;
        }
        pass.set_pipeline(&self.particle_pipeline);
        pass.set_vertex_buffer(0, slice);
        pass.set_bind_group(1, &self.particle_texture, &[]);
        pass.set_bind_group(2, &self.model_group, &[0]);
        pass.draw(0..self.particle_count, 0..1);
    }

    fn draw_flat(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(slice) = self.flat.slice() else {
            return;
        };
        pass.set_vertex_buffer(0, slice);
        pass.set_bind_group(2, &self.model_group, &[0]);
        for draw in &self.flat_draws {
            let pic = match draw.pic {
                Some(index) => self.pics.get(index),
                None => Some(&self.white),
            };
            let pipeline = self.flat_pipelines.get(usize::from(draw.blend));
            if let (Some(pic), Some(pipeline)) = (pic, pipeline) {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(1, pic, &[]);
                pass.draw(draw.first..draw.first.saturating_add(draw.count), 0..1);
            }
        }
    }
}

/// `Fog_SetupFrame`: `glFogf (GL_FOG_DENSITY, Fog_GetDensity () / 64.0)` (REND-10.4).
const REND_FOG_DENSITY_SCALE: f32 = 64.0;

/// Each matrix at its [`MODEL_STRIDE`], column by column.
fn model_bytes(matrices: &[[[f32; 4]; 4]]) -> Vec<u8> {
    let stride = usize::try_from(MODEL_STRIDE).unwrap_or(256);
    let mut bytes = vec![0; matrices.len().saturating_mul(stride)];
    for (chunk, matrix) in bytes.chunks_mut(stride).zip(matrices) {
        let floats: Vec<u8> = matrix
            .iter()
            .flatten()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        for (b, f) in chunk.iter_mut().zip(floats) {
            *b = f;
        }
    }
    bytes
}

/// The dynamic offset of model `model`'s matrix.
fn model_offset(model: usize) -> u32 {
    u32::try_from(model)
        .ok()
        .and_then(|m| m.checked_mul(u32::try_from(MODEL_STRIDE).unwrap_or(256)))
        .unwrap_or(0)
}

/// `vertices` as the alias pipeline reads them: position, skin coordinates and shade.
fn alias_bytes(vertices: &[AliasVertex]) -> Vec<u8> {
    vertices
        .iter()
        .flat_map(|v| {
            let [x, y, z] = v.position;
            let [s, t] = v.texture;
            [x, y, z, s, t, v.shade]
        })
        .flat_map(f32::to_ne_bytes)
        .collect()
}

/// `vertices` as the 2D pipelines read them: position, texture coordinates and colour.
fn flat_bytes(vertices: &[FlatVertex]) -> Vec<u8> {
    vertices
        .iter()
        .flat_map(|v| {
            let [x, y] = v.position;
            let [s, t] = v.texture;
            let [r, g, b, a] = v.color;
            [x, y, s, t, r, g, b, a]
        })
        .flat_map(f32::to_ne_bytes)
        .collect()
}

/// The size a buffer of `size` bytes, or none yet, grows to for `needed` bytes; `None` when it
/// holds them already. A buffer is never smaller than one vertex of `vertex` bytes.
fn grown(size: Option<u64>, needed: u64, vertex: u64) -> Option<u64> {
    let needed = needed.max(vertex);
    size.is_none_or(|size| size < needed).then_some(needed)
}

/// REND-1.11: `glDepthFunc (GL_LEQUAL)`, so of two surfaces at one depth the later drawn,
/// an entity's over the world's, is seen.
const DEPTH_COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::LessEqual;

/// The depth state of a `pass`: tested and written, except that the particles do not write
/// (`glDepthMask (GL_FALSE)`) and the 2D layer is not tested (`glDisable (GL_DEPTH_TEST)`
/// in `GL_Set2D`); a decal is biased toward the eye (REND-11.2).
fn depth_stencil(format: TextureFormat, pass: Pass) -> wgpu::DepthStencilState {
    let (write, compare, bias) = match pass {
        Pass::Opaque | Pass::Blended => (true, DEPTH_COMPARE, wgpu::DepthBiasState::default()),
        Pass::BlendedNoDepthWrite => (false, DEPTH_COMPARE, wgpu::DepthBiasState::default()),
        Pass::Flat => (
            false,
            wgpu::CompareFunction::Always,
            wgpu::DepthBiasState::default(),
        ),
        Pass::Decal => (true, DEPTH_COMPARE, DECAL_BIAS),
    };
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: wgpu::StencilState::default(),
        bias,
    }
}

/// `glBlendFunc (GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA)` where a pass blends.
const fn blend_of(pass: Pass) -> Option<wgpu::BlendState> {
    match pass {
        Pass::Opaque | Pass::Decal => None,
        Pass::Blended | Pass::BlendedNoDepthWrite | Pass::Flat => {
            Some(wgpu::BlendState::ALPHA_BLENDING)
        }
    }
}

/// REND-1.10: GLQuake's `glCullFace (GL_FRONT)` with `GL_CULL_FACE` on and GL's
/// counter-clockwise front face: only the side a face is seen from the front is drawn. The
/// 2D layer is not culled (`glDisable (GL_CULL_FACE)` in `GL_Set2D`), nor are sprites and
/// particles, which face the eye either way.
fn primitive(pass: Pass) -> wgpu::PrimitiveState {
    let cull = match pass {
        Pass::Opaque | Pass::Blended => Some(wgpu::Face::Front),
        Pass::BlendedNoDepthWrite | Pass::Flat | Pass::Decal => None,
    };
    wgpu::PrimitiveState {
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: cull,
        ..wgpu::PrimitiveState::default()
    }
}

/// REND-3.3: world textures are filtered `GL_LINEAR_MIPMAP_NEAREST` when smaller and
/// `GL_LINEAR` when larger, and repeat.
fn world_sampler() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("qs-render world texture"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..wgpu::SamplerDescriptor::default()
    }
}

/// A sampler filtering with `filter`, its coordinates addressed by `address`.
fn sampler(device: &Device, filter: wgpu::FilterMode, address: wgpu::AddressMode) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("qs-render"),
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: filter,
        min_filter: filter,
        ..wgpu::SamplerDescriptor::default()
    })
}

/// The skybox's faces as one texture of six layers, a black texel each with no skybox
/// (REND-8.2).
fn upload_skybox(device: &Device, queue: &Queue, skybox: Option<&Skybox>) -> wgpu::TextureView {
    let [width, height] = skybox.map_or([1, 1], Skybox::size);
    let data = skybox.map_or_else(
        || [0, 0, 0, 255].repeat(usize::try_from(SKY_FACES).unwrap_or(0)),
        Skybox::layers,
    );
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("qs-render skybox"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: SKY_FACES,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &data,
        )
        .create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
}

/// `image` as a texture's view, with no mipmaps.
fn upload(device: &Device, queue: &Queue, image: &Image) -> wgpu::TextureView {
    upload_levels(device, queue, core::slice::from_ref(image))
}

/// The mip levels `levels`, largest first, as one texture's view.
fn upload_levels(device: &Device, queue: &Queue, levels: &[Image]) -> wgpu::TextureView {
    upload_texture(device, queue, levels, wgpu::TextureUsages::TEXTURE_BINDING)
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// The mip levels `levels`, largest first, as one texture used as `usage` says.
fn upload_texture(
    device: &Device,
    queue: &Queue,
    levels: &[Image],
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    let first = levels.first();
    let data: Vec<u8> = levels.iter().flat_map(|l| l.rgba.iter().copied()).collect();
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("qs-render"),
            size: wgpu::Extent3d {
                width: first.map_or(1, |l| l.width),
                height: first.map_or(1, |l| l.height),
                depth_or_array_layers: 1,
            },
            mip_level_count: u32::try_from(levels.len()).unwrap_or(1).max(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        &data,
    )
}

/// The fog over the frame, `Fog_GetColor` and `Fog_GetDensity` (REND-10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    /// The colour, each channel 0 to 1.
    pub color: [f32; 3],
    /// The density before the division by 64; 0 for none.
    pub density: f32,
}

impl Default for Fog {
    /// REND-10.1: no fog, grey.
    fn default() -> Self {
        Self {
            color: [REND_FOG_DEFAULT_GRAY; 3],
            density: 0.0,
        }
    }
}

/// `DEFAULT_GRAY` (REND-10.1).
const REND_FOG_DEFAULT_GRAY: f32 = 0.3;

/// What one frame of the map view is drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The matrix from map coordinates to clip space.
    pub clip_from_world: [[f32; 4]; 4],
    /// The eye's position, which the sky is seen from (REND-2.3).
    pub eye: [f32; 3],
    /// Seconds since the map started, which moving textures follow (REND-2).
    pub time: f32,
    /// The fog (REND-10).
    pub fog: Fog,
    /// `r_wateralpha` (REND-9.1).
    pub water_alpha: f32,
    /// The fog's share of the sky, `skyfog` (REND-8.1).
    pub skyfog: f32,
    /// The 2D layer's width and height in its units, `vid.width` and `vid.height` (HUD-1).
    pub screen: [f32; 2],
}

impl View {
    /// A view through `clip_from_world` from `eye` at `time`, with no fog, opaque water and
    /// a 2D layer of GLQuake's default 640 by 480 (`-conwidth`).
    #[must_use]
    pub fn new(clip_from_world: [[f32; 4]; 4], eye: [f32; 3], time: f32) -> Self {
        Self {
            clip_from_world,
            eye,
            time,
            fog: Fog::default(),
            water_alpha: REND_WATERALPHA,
            skyfog: REND_SKYFOG,
            screen: [CONWIDTH, CONWIDTH * 3.0 / 4.0],
        }
    }
}

/// `r_wateralpha`'s default (REND-9.1).
const REND_WATERALPHA: f32 = 1.0;
/// `r_skyfog`'s default (REND-8.1).
pub(crate) const REND_SKYFOG: f32 = 0.5;
/// `vid.conwidth = 640` when `-conwidth` is not given; `vid.conheight = vid.conwidth*3 / 4`.
const CONWIDTH: f32 = 640.0;

/// One frame of the map view, set before egui's render pass and drawn inside it from the
/// [`MapRenderer`] in egui's callback resources.
pub struct MapCallback {
    /// What it is drawn with.
    pub view: View,
    /// The alias models it shows.
    pub models: ModelFrame,
    /// The view model (REND-4.11).
    pub weapon: ModelFrame,
    /// The sprites (REND-11).
    pub sprites: SpriteFrame,
    /// The particles (PART-3.2).
    pub particles: Vec<ParticleVertex>,
    /// The 2D layer: the view blend, then the status bar and the centre print.
    pub flat: FlatFrame,
    /// Each of the mesh's models' matrices, the world's first; `None` for one not drawn.
    pub placements: Vec<Option<[[f32; 4]; 4]>>,
    /// The lightmap made again for this frame's light styles, when they changed (REND-5.5).
    pub lightmap: Option<Image>,
    /// The translated skin of each player drawn this frame (REND-4.13).
    pub player_skins: Vec<PlayerSkin>,
}

impl CallbackTrait for MapCallback {
    fn prepare(
        &self,
        device: &Device,
        queue: &Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<MapRenderer>() {
            renderer.set_view(queue, &self.view);
            renderer.set_models(device, queue, &self.models);
            renderer.set_weapon(device, queue, &self.weapon);
            renderer.set_sprites(device, queue, &self.sprites);
            renderer.set_particles(device, queue, &self.particles);
            renderer.set_flat(device, queue, &self.flat);
            if !self.placements.is_empty() {
                renderer.set_placements(queue, &self.placements);
            }
            if let Some(lightmap) = &self.lightmap {
                renderer.set_lightmap(queue, lightmap);
            }
            for skin in &self.player_skins {
                renderer.set_player_skin(device, queue, skin);
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: epaint::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        if let Some(renderer) = resources.get::<MapRenderer>() {
            let viewport = info.viewport_in_pixels();
            let viewport = [
                viewport.left_px,
                viewport.top_px,
                viewport.width_px,
                viewport.height_px,
            ]
            .map(|v| f32_of_i32(v.max(0)));
            let viewport = (viewport[2] > 0.0 && viewport[3] > 0.0).then_some(viewport);
            renderer.draw(pass, self.view.time, viewport);
        }
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a viewport's pixels, far below f32's exact range"
)]
const fn f32_of_i32(value: i32) -> f32 {
    value as f32
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a skybox face's pixels, far below f32's exact range"
)]
const fn f32_of_u32(value: u32) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "test code: a failed unwrap or index is a failed test; the image's pixels are small, exact numbers"
    )]

    use egui_wgpu::wgpu;
    use egui_wgpu::wgpu::naga::front::wgsl;
    use egui_wgpu::wgpu::naga::valid::{Capabilities, ValidationFlags, Validator};

    use qs_bsp::{SURF_DRAWSKY, SURF_DRAWTURB};

    use super::{
        ALIAS_VERTEX_BYTES, AliasDraw, Assets, DEPTH_COMPARE, FlatDraw, FlatFrame, FlatVertex, Fog,
        MapCallback, MapRenderer, ModelFrame, Pass, SpriteDraw, SpriteFrame, View, alias_bytes,
        grown, primitive,
    };
    use crate::alias::AliasVertex;
    use crate::camera::Camera;
    use crate::mesh::Image;
    use crate::mesh::tests::{MapSpec, bsp, palette, samples};
    use crate::particles::ParticleVertex;
    use crate::picture::tests::{TgaSpec, solid, tga};
    use crate::skybox::Skybox;
    use crate::sprite::SpriteVertex;
    use crate::{LightStyles, MapMesh};
    use egui_wgpu::CallbackTrait as _;

    const FORMATS: (wgpu::TextureFormat, wgpu::TextureFormat) = (
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Depth32Float,
    );

    /// A headless device, and a renderer of the test map on it, lit by 50 as the lightmap
    /// tests read it.
    fn renderer() -> (egui_wgpu::RenderState, MapRenderer, MapMesh) {
        renderer_of(&MapSpec::default(), &[50], &Assets::default())
    }

    /// A headless device, and a renderer of the test map `spec` lit by `lighting` on it, with
    /// `assets`.
    fn renderer_of(
        spec: &MapSpec,
        lighting: &[u8],
        assets: &Assets<'_>,
    ) -> (egui_wgpu::RenderState, MapRenderer, MapMesh) {
        let state = egui_kittest::wgpu::create_render_state(
            egui_kittest::wgpu::default_wgpu_setup(),
            egui_wgpu::RendererOptions::default(),
        );
        let lighting = samples(lighting);
        let mesh = MapMesh::build(&bsp(spec, &lighting), &palette()).unwrap();
        let renderer = MapRenderer::new(&state.device, &state.queue, FORMATS, &mesh, assets);
        (state, renderer, mesh)
    }

    /// The test face wound so its front, which GLQuake culls (REND-1.10), is below: seen
    /// from above.
    fn seen_from_above() -> MapSpec {
        MapSpec {
            surfedges: vec![-4, -3, -2, -1],
            ..MapSpec::default()
        }
    }

    /// The renderer of the test map at full light, seen from above, for the offscreen images.
    fn bright() -> (egui_wgpu::RenderState, MapRenderer, MapMesh) {
        renderer_of(&seen_from_above(), &[255], &Assets::default())
    }

    /// The side of the offscreen image: 64 pixels, whose rows are 256 bytes, the copy
    /// alignment.
    const SIDE: u32 = 64;

    /// A view from 40 above the test face's middle looking straight down, so the face fills
    /// the middle of the image: the pixel at (32, 32) is the face's (32, 16).
    fn looking_down() -> View {
        View::new(
            Camera::clip_from_view([32.0, 16.0, 40.0], [90.0, 0.0, 0.0], 90.0, 1.0),
            [32.0, 16.0, 40.0],
            0.0,
        )
    }

    /// A frame with nothing but the world, looking down at it.
    fn frame() -> MapCallback {
        MapCallback {
            view: looking_down(),
            models: ModelFrame::default(),
            weapon: ModelFrame::default(),
            sprites: SpriteFrame::default(),
            particles: Vec::new(),
            flat: FlatFrame::default(),
            placements: Vec::new(),
            lightmap: None,
            player_skins: Vec::new(),
        }
    }

    /// Draws `callback` offscreen over `clear` and returns the image's pixels, four bytes
    /// each, row by row.
    fn render(
        state: &egui_wgpu::RenderState,
        renderer: MapRenderer,
        callback: &MapCallback,
        clear: wgpu::Color,
    ) -> Vec<u8> {
        let mut resources = egui_wgpu::CallbackResources::default();
        resources.insert(renderer);
        let texture = |format, usage| {
            state.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: SIDE,
                    height: SIDE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let colour = texture(
            FORMATS.0,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(FORMATS.1, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let colour_view = colour.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = state.device.create_command_encoder(&Default::default());
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [SIDE, SIDE],
            pixels_per_point: 1.0,
        };
        callback.prepare(
            &state.device,
            &state.queue,
            &screen,
            &mut encoder,
            &mut resources,
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &colour_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let renderer = resources.get::<MapRenderer>().unwrap();
            let side = f32::from(u16::try_from(SIDE).unwrap());
            renderer.draw(&mut pass, callback.view.time, Some([0.0, 0.0, side, side]));
        }
        let bytes = u64::from(SIDE) * u64::from(SIDE) * 4;
        let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            colour.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIDE * 4),
                    rows_per_image: Some(SIDE),
                },
            },
            colour.size(),
        );
        state.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        state
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        buffer.slice(..).get_mapped_range().unwrap().to_vec()
    }

    /// The pixel at `(x, y)` of an image `render` returned.
    fn pixel(image: &[u8], x: u32, y: u32) -> [u8; 4] {
        let at = usize::try_from((y * SIDE + x) * 4).unwrap();
        image[at..at + 4].try_into().unwrap()
    }

    /// Whether each channel of `a` is within `by` of `b`'s.
    fn near(a: [u8; 4], b: [u8; 4], by: u8) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= by)
    }

    const BLACK: wgpu::Color = wgpu::Color::BLACK;
    /// The test face's colour: texture index 7 through the test palette, full bright.
    const FACE: [u8; 4] = [7, 14, 21, 255];

    /// `v` as a byte, rounded.
    fn byte(v: f32) -> u8 {
        u8::try_from(v.round() as i32).unwrap()
    }

    // spec REND-1.7: the face is drawn where the view sees it, its texture times its light
    #[test]
    fn the_world_is_drawn_where_the_view_sees_it() {
        let (state, renderer, _) = bright();
        let image = render(&state, renderer, &frame(), BLACK);
        assert_eq!(pixel(&image, 32, 32), FACE);
        // The view spans x -8 to 72 and y -24 to 56: the face's corner (0, 0) is 8 units from
        // the view's left edge and 24 from its top, 6 and 19 pixels; beyond it is the clear.
        assert_eq!(pixel(&image, 2, 2), [0, 0, 0, 255]);
    }

    // spec REND-4.13: a player skin is uploaded when its key is not the one its slot holds;
    // a slot past the sixteen is ignored
    #[test]
    fn player_skins_are_uploaded_when_their_key_changes() {
        let (state, mut renderer, _) = renderer();
        let image = std::sync::Arc::new(Image {
            width: 1,
            height: 1,
            rgba: vec![9; 4],
        });
        let skin = |slot, colors| super::PlayerSkin {
            slot,
            key: super::SkinKey {
                model: 1,
                skin: 0,
                colors,
            },
            image: image.clone(),
        };
        assert_eq!(renderer.player_keys[3], None);
        renderer.set_player_skin(&state.device, &state.queue, &skin(3, 0x40));
        assert_eq!(renderer.player_keys[3].map(|k| k.colors), Some(0x40));
        renderer.set_player_skin(&state.device, &state.queue, &skin(3, 0x50));
        assert_eq!(renderer.player_keys[3].map(|k| k.colors), Some(0x50));
        renderer.set_player_skin(&state.device, &state.queue, &skin(16, 0x40));
        assert!(renderer.player_keys.iter().filter(|k| k.is_some()).count() == 1);
    }

    /// The colour of each quadrant of the test skybox's `dn` face, left to right, top to
    /// bottom.
    const QUADRANTS: [[u8; 4]; 4] = [
        [200, 0, 0, 255],
        [0, 200, 0, 255],
        [0, 0, 200, 255],
        [200, 200, 0, 255],
    ];

    /// A skybox whose faces are each one grey, `rt` 10, `bk` 20, `lf` 30, `ft` 40 and `up` 50,
    /// but `dn`, 64 by 64 in [`QUADRANTS`].
    fn skybox() -> Skybox {
        let mut dn = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                dn.extend(QUADRANTS[usize::from(y >= 32) * 2 + usize::from(x >= 32)]);
            }
        }
        let read = move |name: &str| match name {
            "gfx/env/tdn.tga" => Some(tga(&TgaSpec {
                width: 64,
                height: 64,
                rgba: dn.clone(),
                top_down: true,
            })),
            _ => {
                let side = ["rt", "bk", "lf", "ft", "up"]
                    .iter()
                    .position(|side| name == format!("gfx/env/t{side}.tga"))?;
                let grey = u8::try_from(side + 1).unwrap() * 10;
                Some(solid([64, 64], [grey, grey, grey, 255]))
            }
        };
        Skybox::load("t", &read).unwrap()
    }

    /// The test face as sky, seen from above, drawn with `skybox`'s faces.
    fn sky_frame(skybox: &Skybox, view: View) -> Vec<u8> {
        let spec = MapSpec {
            flags: SURF_DRAWSKY,
            ..seen_from_above()
        };
        let assets = Assets {
            skybox: Some(skybox),
            ..Assets::default()
        };
        let (state, renderer, _) = renderer_of(&spec, &[255], &assets);
        let callback = MapCallback { view, ..frame() };
        render(&state, renderer, &callback, BLACK)
    }

    // spec REND-8.3: looking down, the sky face shows the `dn` face, its `vec_to_st` s from
    // -y and t from x. The view's up is +x and its right -y (`AngleVectors` at pitch 90), 1.25
    // units a pixel at 40 below: pixel (20, 25) is world (40.75, 31), s -0.375 and t 0.22, the
    // top left quadrant; pixel (44, 40) is (22, 1), s 0.375 and t -0.25, the bottom right
    #[test]
    fn the_sky_shows_the_skybox_face_it_looks_through() {
        let image = sky_frame(&skybox(), looking_down());
        assert_eq!(pixel(&image, 20, 25), QUADRANTS[0]);
        assert_eq!(pixel(&image, 44, 40), QUADRANTS[3]);
        assert_eq!(pixel(&image, 2, 2), [0, 0, 0, 255], "no sky face there");
    }

    // spec REND-8.3: the face is the direction's largest axis, through `skytexorder`: +x `rt`,
    // -x `lf`, +y `bk`, -y `ft`; each view looks at the sky face's middle from 60 away and
    // 40 above, so the horizontal axis is the largest
    #[test]
    fn each_direction_shows_its_skybox_face() {
        let skybox = skybox();
        for ([dx, dy], yaw, grey) in [
            ([-1.0, 0.0], 0.0, 10),
            ([1.0, 0.0], 180.0, 30),
            ([0.0, -1.0], 90.0, 20),
            ([0.0, 1.0], 270.0, 40),
        ] {
            let eye = [32.0 + 60.0 * dx, 16.0 + 60.0 * dy, 40.0];
            let pitch = (40.0_f32 / 60.0).atan().to_degrees();
            let view = View::new(
                Camera::clip_from_view(eye, [pitch, yaw, 0.0], 90.0, 1.0),
                eye,
                0.0,
            );
            let image = sky_frame(&skybox, view);
            assert_eq!(pixel(&image, 32, 32), [grey, grey, grey, 255], "yaw {yaw}");
        }
    }

    // spec REND-8.1: with fog, `skyfog` of the fog's colour over the skybox; 1 is all fog
    #[test]
    fn the_fog_covers_the_skybox_by_its_share() {
        let skybox = skybox();
        let fogged = |skyfog: f32| {
            let mut view = looking_down();
            view.fog = Fog {
                color: [1.0, 1.0, 1.0],
                density: 0.001,
            };
            view.skyfog = skyfog;
            pixel(&sky_frame(&skybox, view), 20, 25)
        };
        assert_eq!(fogged(0.0), QUADRANTS[0]);
        assert!(
            near(fogged(0.5), [227, 128, 128, 255], 1),
            "{:?}",
            fogged(0.5)
        );
        assert_eq!(fogged(1.0), [255, 255, 255, 255]);
    }

    // spec REND-10.4: a fragment's colour is mixed toward the fog colour by
    // `1 - exp (-((density / 64) * z)^2)` of its distance from the eye
    #[test]
    fn fog_mixes_the_face_toward_the_fog_colour_by_distance() {
        let fogged = |density: f32| {
            let (state, renderer, _) = bright();
            let mut callback = frame();
            callback.view.fog = Fog {
                color: [0.3, 0.6, 0.9],
                density,
            };
            let image = render(&state, renderer, &callback, BLACK);
            pixel(&image, 32, 32)
        };
        assert_eq!(fogged(0.0), FACE);
        // Density 64 at 40 units: `1 - exp (-1600)`, all fog.
        let all = fogged(64.0);
        assert!(near(all, [77, 153, 230, 255], 1), "{all:?}");
        // Density 1 at 40 units: `1 - exp (-(40/64)^2)` = 0.323 of the way.
        let mixed = fogged(1.0);
        let mix = 1.0 - (-(40.0_f32 / 64.0).powi(2)).exp();
        let expect = |face: f32, fog: f32| byte(face + (fog - face) * mix);
        let want = [
            expect(7.0, 76.5),
            expect(14.0, 153.0),
            expect(21.0, 229.5),
            255,
        ];
        assert!(near(mixed, want, 2), "{mixed:?} vs {want:?}");
    }

    // spec REND-9.1: water is blended over the scene with the water alpha, 1 by default
    #[test]
    fn water_blends_by_the_water_alpha() {
        let water = |alpha: f32| {
            let spec = MapSpec {
                flags: SURF_DRAWTURB,
                ..seen_from_above()
            };
            let (state, renderer, _) = renderer_of(&spec, &[255], &Assets::default());
            let mut callback = frame();
            callback.view.water_alpha = alpha;
            let image = render(&state, renderer, &callback, wgpu::Color::WHITE);
            pixel(&image, 32, 32)
        };
        // Turbulent water samples its texture at warped coordinates: the one texel colour.
        assert_eq!(water(1.0), FACE);
        let half = water(0.5);
        assert!(near(half, [131, 134, 138, 255], 2), "{half:?}");
    }

    // spec REND-11.2: a sprite's quad is drawn with its frame image, transparent texels cut
    // by the alpha test
    #[test]
    fn sprites_are_alpha_tested() {
        // A 4 by 1 frame: red twice, then transparent twice.
        let frames = [Image {
            width: 4,
            height: 1,
            rgba: vec![255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0],
        }];
        let assets = Assets {
            frames: &frames,
            ..Assets::default()
        };
        let (state, renderer, _) = renderer_of(&seen_from_above(), &[255], &assets);
        let mut callback = frame();
        // A quad 20 above the face, from x 0 to 64 and y 0 to 32, textured along x.
        let corner = |x: f32, y: f32, s: f32, t: f32| SpriteVertex {
            position: [x, y, 20.0],
            texture: [s, t],
        };
        let quad = [
            corner(0.0, 0.0, 0.0, 1.0),
            corner(0.0, 32.0, 0.0, 0.0),
            corner(64.0, 32.0, 1.0, 0.0),
            corner(64.0, 0.0, 1.0, 1.0),
        ];
        callback.sprites = SpriteFrame {
            vertices: vec![quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]],
            draws: vec![SpriteDraw {
                image: 0,
                first: 0,
                count: 6,
                oriented: false,
            }],
        };
        let out = render(&state, renderer, &callback, BLACK);
        // Looking down with the view's up along +x, x runs up the image: 20 below the eye the
        // view spans 20 units each way, so x 24 is 13 pixels below the middle, at the second
        // red texel's centre, and x 40 is 13 above it, at the first transparent texel's,
        // where the face shows.
        assert_eq!(pixel(&out, 32, 45), [255, 0, 0, 255]);
        assert_eq!(pixel(&out, 32, 19), FACE);
    }

    // spec PART-3.2: a particle's triangle is drawn in its colour through the particle
    // texture, blended
    #[test]
    fn particles_are_drawn_in_their_colour() {
        let (state, renderer, _) = bright();
        let mut callback = frame();
        let corner = |x: f32, y: f32| ParticleVertex {
            position: [x, y, 20.0],
            // Inside the dot: texel (1, 1) of 8.
            texture: [0.1875, 0.1875],
            color: [0.0, 1.0, 0.0, 1.0],
        };
        callback.particles = vec![
            corner(-50.0, -50.0),
            corner(200.0, -50.0),
            corner(-50.0, 200.0),
        ];
        let out = render(&state, renderer, &callback, BLACK);
        assert_eq!(pixel(&out, 32, 32), [0, 255, 0, 255]);
    }

    // spec REND-4.11: the view model is drawn in the nearest 0.3 of the depth range, so it
    // is seen through a wall an alias model is hidden by
    #[test]
    fn the_view_model_is_drawn_over_the_walls() {
        let skin = [Image {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        }];
        let assets = Assets {
            skins: &skin,
            ..Assets::default()
        };
        // A triangle 10 below the face, behind it from the eye.
        let corner = |x: f32, y: f32| AliasVertex {
            position: [x, y, -10.0],
            texture: [0.5, 0.5],
            shade: 1.0,
        };
        // Wound as a Quake face is, so the front culled by REND-1.10 is away from the eye.
        let model = ModelFrame {
            vertices: vec![
                corner(-50.0, -50.0),
                corner(-50.0, 200.0),
                corner(200.0, -50.0),
            ],
            draws: vec![AliasDraw {
                skin: 0,
                first: 0,
                count: 3,
            }],
        };
        let (state, renderer, _) = renderer_of(&seen_from_above(), &[255], &assets);
        let mut callback = frame();
        callback.models = model.clone();
        let hidden = render(&state, renderer, &callback, BLACK);
        assert_eq!(pixel(&hidden, 32, 32), FACE, "an entity behind the face");
        let (state, renderer, _) = renderer_of(&seen_from_above(), &[255], &assets);
        let mut callback = frame();
        callback.weapon = model;
        let shown = render(&state, renderer, &callback, BLACK);
        assert_eq!(
            pixel(&shown, 32, 32),
            [255, 255, 255, 255],
            "the view model"
        );
    }

    // spec HUD-1.1: the 2D layer is drawn in its own units from the top left over the view,
    // `glOrtho (0, vid.width, vid.height, 0, ...)`
    // spec VIEW-4.6: the view blend is blended over the frame by its alpha
    #[test]
    fn the_flat_layer_covers_the_view_in_screen_units() {
        let (state, renderer, _) = bright();
        let mut callback = frame();
        let corner = |x: f32, y: f32, color| FlatVertex {
            position: [x, y],
            texture: [0.5, 0.5],
            color,
        };
        let red = [1.0, 0.0, 0.0, 1.0];
        let blue = [0.0, 0.0, 1.0, 0.5];
        // A fill over the top-left quarter, and a half blue blend over the whole layer.
        let quad = |w: f32, h: f32, color| {
            [
                corner(0.0, 0.0, color),
                corner(w, 0.0, color),
                corner(w, h, color),
                corner(0.0, 0.0, color),
                corner(w, h, color),
                corner(0.0, h, color),
            ]
        };
        let mut vertices = quad(320.0, 240.0, red).to_vec();
        vertices.extend(quad(640.0, 480.0, blue));
        callback.flat = FlatFrame {
            vertices,
            draws: vec![
                FlatDraw {
                    pic: None,
                    blend: false,
                    first: 0,
                    count: 6,
                },
                FlatDraw {
                    pic: None,
                    blend: true,
                    first: 6,
                    count: 6,
                },
            ],
        };
        let out = render(&state, renderer, &callback, BLACK);
        // Top left: red, then half blue over it.
        let top_left = pixel(&out, 16, 16);
        assert!(near(top_left, [128, 0, 128, 255], 1), "{top_left:?}");
        // Bottom right, off the face: the clear, then half blue.
        let bottom_right = pixel(&out, 60, 60);
        assert!(near(bottom_right, [0, 0, 128, 255], 1), "{bottom_right:?}");
        // Over the face: the face, then half blue.
        let over = pixel(&out, 40, 40);
        assert!(near(over, [4, 7, 138, 255], 2), "{over:?}");
    }

    /// The renderer's lightmap texels, read back from the GPU, first row.
    fn lightmap_row(state: &egui_wgpu::RenderState, renderer: &MapRenderer) -> Vec<u8> {
        let size = renderer.lightmap.size();
        let row = u64::from(size.width) * 4;
        let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: row,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = state.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            renderer.lightmap.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(u32::try_from(row).unwrap()),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d { height: 1, ..size },
        );
        state.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        state
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        buffer.slice(..).get_mapped_range().unwrap().to_vec()
    }

    // spec REND-5.5: a lightmap made again for new styles is written over the one drawn; one
    // of another size is not
    #[test]
    fn a_new_lightmap_is_written_over_the_old() {
        let (state, renderer, mut mesh) = renderer();
        assert_eq!(&lightmap_row(&state, &renderer)[4..8], [100, 100, 100, 255]);
        let off = LightStyles::at(|s| (s == 0).then_some(b"a".as_slice()), 0.0);
        assert!(mesh.relight(&off));
        renderer.set_lightmap(&state.queue, &mesh.lightmap);
        assert_eq!(&lightmap_row(&state, &renderer)[4..8], [0, 0, 0, 255]);
        let small = Image {
            width: 1,
            height: 1,
            rgba: vec![9; 4],
        };
        renderer.set_lightmap(&state.queue, &small);
        assert_eq!(&lightmap_row(&state, &renderer)[..4], [255, 255, 255, 255]);
    }

    // spec REND-5.5: the paint callback writes the lightmap it carries before drawing
    #[test]
    fn the_callback_writes_its_lightmap() {
        let (state, renderer, mut mesh) = renderer();
        let off = LightStyles::at(|s| (s == 0).then_some(b"a".as_slice()), 0.0);
        mesh.relight(&off);
        let mut resources = egui_wgpu::CallbackResources::default();
        resources.insert(renderer);
        let callback = MapCallback {
            view: View::new([[0.0; 4]; 4], [0.0; 3], 0.0),
            models: ModelFrame::default(),
            weapon: ModelFrame::default(),
            sprites: SpriteFrame::default(),
            particles: Vec::new(),
            flat: FlatFrame::default(),
            placements: Vec::new(),
            lightmap: Some(mesh.lightmap.clone()),
            player_skins: Vec::new(),
        };
        let mut encoder = state.device.create_command_encoder(&Default::default());
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [1, 1],
            pixels_per_point: 1.0,
        };
        let commands = callback.prepare(
            &state.device,
            &state.queue,
            &screen,
            &mut encoder,
            &mut resources,
        );
        assert!(commands.is_empty());
        state.queue.submit([encoder.finish()]);
        let renderer = resources.get::<MapRenderer>().unwrap();
        assert_eq!(&lightmap_row(&state, renderer)[4..8], [0, 0, 0, 255]);
    }

    // spec REND-1.11: a fragment at the depth already there passes, GL_LEQUAL
    #[test]
    fn depth_ties_go_to_the_later_surface() {
        assert_eq!(DEPTH_COMPARE, wgpu::CompareFunction::LessEqual);
    }

    // spec REND-1.10: only a face's front is drawn: GL's counter-clockwise front, front culled
    #[test]
    fn faces_are_culled_as_glquake_culls_them() {
        let state = primitive(Pass::Opaque);
        assert_eq!(state.front_face, wgpu::FrontFace::Ccw);
        assert_eq!(state.cull_mode, Some(wgpu::Face::Front));
        assert_eq!(state.topology, wgpu::PrimitiveTopology::TriangleList);
        // `GL_Set2D` turns culling off; sprites and particles face the eye either way.
        for pass in [Pass::Flat, Pass::Decal, Pass::BlendedNoDepthWrite] {
            assert_eq!(primitive(pass).cull_mode, None);
        }
    }
    #[test]
    fn alias_vertices_are_six_floats_in_order() {
        let vertex = AliasVertex {
            position: [1.0, 2.0, 3.0],
            texture: [4.0, 5.0],
            shade: 6.0,
        };
        let bytes = alias_bytes(&[vertex, vertex]);
        assert_eq!(u64::try_from(bytes.len()).unwrap(), 2 * ALIAS_VERTEX_BYTES);
        let floats: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_ne_bytes(*b))
            .take(6)
            .collect();
        assert_eq!(floats, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn the_alias_buffer_grows_only_when_it_must() {
        let vertex = ALIAS_VERTEX_BYTES;
        assert_eq!(grown(None, 0, vertex), Some(ALIAS_VERTEX_BYTES));
        assert_eq!(grown(None, 100, vertex), Some(100));
        assert_eq!(grown(Some(100), 100, vertex), None);
        assert_eq!(grown(Some(100), 50, vertex), None);
        assert_eq!(grown(Some(100), 101, vertex), Some(101));
    }

    const SHADER: &str = include_str!("shader.wgsl");

    // spec REND-1.7: the surface is its texture's colour times the lightmap's light, in a
    // shader every backend accepts
    // spec REND-6.2: each channel by the lightmap's own channel
    #[test]
    fn the_shader_validates_and_darkens_the_texture_by_the_light() {
        let module = wgsl::parse_str(SHADER).unwrap();
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .unwrap();
        assert!(
            SHADER.contains(
                "let light = textureSample(lightmap, lightmap_sampler, input.light).rgb;"
            )
        );
        assert!(
            SHADER.contains("return vec4<f32>(fogged(colour.rgb * light, input.world), OPAQUE);")
        );
    }

    // spec REND-4.8: an alias model's skin is multiplied by its vertex's shade
    #[test]
    fn the_shader_modulates_a_skin_by_its_shade() {
        let module = wgsl::parse_str(SHADER).unwrap();
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .unwrap();
        assert!(SHADER.contains("fogged(colour.rgb * input.shade, input.world), OPAQUE);"));
    }

    // spec REND-2.2: water ripples by 8 sin of `t*0.125 + realtime`, over 64
    // spec REND-2.3: the solid layer scrolls at 8 a second in the right half, the alpha layer
    // at 16 in the left half over it
    // spec REND-2.5: both are computed for each pixel
    #[test]
    fn the_shader_moves_water_and_sky() {
        let module = wgsl::parse_str(SHADER).unwrap();
        let entries: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(
            entries,
            [
                "vertex",
                "fragment",
                "turbulent",
                "sky",
                "alias_vertex",
                "alias_fragment",
                "sprite_vertex",
                "sprite_fragment",
                "particle_vertex",
                "particle_fragment",
                "flat_vertex",
                "flat_fragment",
                "blend_fragment"
            ]
        );
        for constant in [
            "const TURB_AMPLITUDE: f32 = 8.0;",
            "const TURB_FREQUENCY: f32 = 0.125;",
            "const TURB_TILE: f32 = 64.0;",
            "const SKY_FLATTEN: f32 = 3.0;",
            "const SKY_RADIUS: f32 = 378.0;",
            "const SKY_TILE: f32 = 128.0;",
            "const SKY_SOLID_SPEED: f32 = 8.0;",
            "const SKY_ALPHA_SPEED: f32 = 16.0;",
            "const SKY_SCROLL_WRAP: f32 = 128.0;",
        ] {
            assert!(SHADER.contains(constant), "{constant}");
        }
        assert!(
            SHADER.contains(
                "(st.x + TURB_AMPLITUDE * sin(st.y * TURB_FREQUENCY + time)) / TURB_TILE"
            )
        );
        assert!(SHADER.contains("mix(solid.rgb, alpha.rgb, alpha.a)"));
        assert!(SHADER.contains("sky_st(flat, time * SKY_SOLID_SPEED)"));
        assert!(SHADER.contains(
            "textureSample(sky_alpha, surface_sampler, sky_st(flat, time * SKY_ALPHA_SPEED))"
        ));
    }

    // spec REND-3.3: world textures are linear within the nearest mip level, repeating
    #[test]
    fn world_textures_filter_as_gl_linear_mipmap_nearest() {
        use egui_wgpu::wgpu::{AddressMode, FilterMode, MipmapFilterMode};
        let world = super::world_sampler();
        assert_eq!(
            (world.min_filter, world.mag_filter, world.mipmap_filter),
            (
                FilterMode::Linear,
                FilterMode::Linear,
                MipmapFilterMode::Nearest
            )
        );
        assert_eq!(
            (world.address_mode_u, world.address_mode_v),
            (AddressMode::Repeat, AddressMode::Repeat)
        );
    }

    // Every number in the shader's code is a named constant: none is written after the
    // constants but in a comment.
    #[test]
    fn the_shader_names_its_numbers() {
        let code = SHADER.split("struct View").nth(1).unwrap();
        for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
            assert!(
                !line.chars().any(|c| c.is_ascii_digit())
                    || line.contains("@location")
                    || line.contains("@binding")
                    || line.contains("@group")
                    || line.contains("vec2<f32>")
                    || line.contains("vec3<f32>")
                    || line.contains("vec4<f32>")
                    || line.contains("mat4x4<f32>"),
                "a number in: {line}"
            );
        }
    }
}
