// SPDX-License-Identifier: GPL-3.0-or-later

// The world's surfaces: lit (REND-1.7), turbulent (REND-2.2) and sky (REND-2.3); and alias
// models (REND-4.8).

// REND-2.2: the peak of `turbsin`, 8 sin(2πi/256), in `WinQuake/gl_warp_sin.h`.
const TURB_AMPLITUDE: f32 = 8.0;
// REND-2.2: `ot*0.125` and `os*0.125` in `EmitWaterPolys`: the coordinate's share of the
// ripple's angle, in radians per texel.
const TURB_FREQUENCY: f32 = 0.125;
// REND-2.2: `s *= (1.0/64)` in `EmitWaterPolys`: the texels a turbulent texture repeats over.
const TURB_TILE: f32 = 64.0;
// REND-2.3: `dir[2] *= 3` in `EmitSkyPolys`, which "flatten[s] the sphere".
const SKY_FLATTEN: f32 = 3.0;
// REND-2.3: `length = 6*63/length` in `EmitSkyPolys`: the sky dome's radius in texels.
const SKY_RADIUS: f32 = 378.0;
// REND-2.3: `(1.0/128)` in `EmitSkyPolys`: the texels one sky layer repeats over.
const SKY_TILE: f32 = 128.0;
// REND-2.3: `speedscale = realtime*8` for the solid layer in `R_DrawSkyChain`.
const SKY_SOLID_SPEED: f32 = 8.0;
// REND-2.3: `speedscale = realtime*16` for the alpha layer in `R_DrawSkyChain`.
const SKY_ALPHA_SPEED: f32 = 16.0;
// REND-2.3: `speedscale -= (int)speedscale & ~127`: the scroll kept within one layer's 128
// texels, which keeps the coordinates small as time passes.
const SKY_SCROLL_WRAP: f32 = 128.0;
// The shortest distance the sky's direction is divided by, so a point at the eye divides by
// no zero; GLQuake never draws one.
const SKY_MIN_DISTANCE: f32 = 0.001;
// Every surface is drawn opaque: the sky's alpha layer is blended in the shader (REND-2.3).
const OPAQUE: f32 = 1.0;
// The w of a position, which makes it a point the view matrix moves.
const POINT: f32 = 1.0;

struct View {
    clip_from_world: mat4x4<f32>,
    // The eye's position, and the time in seconds.
    eye_time: vec4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var lightmap: texture_2d<f32>;
@group(0) @binding(2) var lightmap_sampler: sampler;
@group(1) @binding(0) var surface: texture_2d<f32>;
@group(1) @binding(1) var surface_sampler: sampler;
// The sky's alpha layer; for any other surface, its texture again.
@group(1) @binding(2) var sky_alpha: texture_2d<f32>;
// The model's matrix: where a brush entity stands now (REND-1.9); the world's moves nothing.
@group(2) @binding(0) var<uniform> model: mat4x4<f32>;

struct In {
    @location(0) position: vec3<f32>,
    @location(1) texture: vec2<f32>,
    @location(2) light: vec2<f32>,
};

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) texture: vec2<f32>,
    @location(1) light: vec2<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vertex(input: In) -> Out {
    var out: Out;
    let world = model * vec4<f32>(input.position, POINT);
    out.clip = view.clip_from_world * world;
    out.texture = input.texture;
    out.light = input.light;
    out.world = world.xyz;
    return out;
}

// REND-1.7: the texture's colour times the light, each channel by its own (REND-6).
@fragment
fn fragment(input: Out) -> @location(0) vec4<f32> {
    let colour = textureSample(surface, surface_sampler, input.texture);
    let light = textureSample(lightmap, lightmap_sampler, input.light).rgb;
    return vec4<f32>(colour.rgb * light, OPAQUE);
}

// REND-2.2: `(s + 8 sin(t*0.125 + realtime)) / 64`, and t the same way round.
@fragment
fn turbulent(input: Out) -> @location(0) vec4<f32> {
    let time = view.eye_time.w;
    let st = input.texture;
    let s = (st.x + TURB_AMPLITUDE * sin(st.y * TURB_FREQUENCY + time)) / TURB_TILE;
    let t = (st.y + TURB_AMPLITUDE * sin(st.x * TURB_FREQUENCY + time)) / TURB_TILE;
    return vec4<f32>(textureSample(surface, surface_sampler, vec2<f32>(s, t)).rgb, OPAQUE);
}

// REND-2.3: the solid layer scrolling under the alpha layer, both seen from the eye.
@fragment
fn sky(input: Out) -> @location(0) vec4<f32> {
    var dir = input.world - view.eye_time.xyz;
    dir.z = dir.z * SKY_FLATTEN;
    let length = SKY_RADIUS / max(sqrt(dot(dir, dir)), SKY_MIN_DISTANCE);
    let flat = dir.xy * length;
    let time = view.eye_time.w;
    let solid = textureSample(surface, surface_sampler, sky_st(flat, time * SKY_SOLID_SPEED));
    let alpha = textureSample(sky_alpha, surface_sampler, sky_st(flat, time * SKY_ALPHA_SPEED));
    return vec4<f32>(mix(solid.rgb, alpha.rgb, alpha.a), OPAQUE);
}

// One sky layer's coordinates, `(speedscale + dir) / 128`, the scroll kept within 128 texels.
fn sky_st(flat: vec2<f32>, scroll: f32) -> vec2<f32> {
    let speedscale = scroll - floor(scroll / SKY_SCROLL_WRAP) * SKY_SCROLL_WRAP;
    return (vec2<f32>(speedscale) + flat) / SKY_TILE;
}

struct AliasIn {
    @location(0) position: vec3<f32>,
    @location(1) texture: vec2<f32>,
    @location(2) shade: f32,
};

struct AliasOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) texture: vec2<f32>,
    // Interpolated across the triangle, `glShadeModel (GL_SMOOTH)` (REND-4.8).
    @location(1) shade: f32,
};

@vertex
fn alias_vertex(input: AliasIn) -> AliasOut {
    var out: AliasOut;
    out.clip = view.clip_from_world * vec4<f32>(input.position, POINT);
    out.texture = input.texture;
    out.shade = input.shade;
    return out;
}

// REND-4.8: the skin's colour times the vertex's shade, `GL_MODULATE`.
@fragment
fn alias_fragment(input: AliasOut) -> @location(0) vec4<f32> {
    let colour = textureSample(surface, surface_sampler, input.texture);
    return vec4<f32>(colour.rgb * input.shade, OPAQUE);
}
