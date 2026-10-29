// SPDX-License-Identifier: GPL-3.0-or-later

// The world's surfaces (REND-1.7): the texture's colour times the lightmap's light / 255.

struct View {
    clip_from_world: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var lightmap: texture_2d<f32>;
@group(0) @binding(2) var lightmap_sampler: sampler;
@group(1) @binding(0) var surface: texture_2d<f32>;
@group(1) @binding(1) var surface_sampler: sampler;

struct In {
    @location(0) position: vec3<f32>,
    @location(1) texture: vec2<f32>,
    @location(2) light: vec2<f32>,
};

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) texture: vec2<f32>,
    @location(1) light: vec2<f32>,
};

@vertex
fn vertex(input: In) -> Out {
    var out: Out;
    out.clip = view.clip_from_world * vec4<f32>(input.position, 1.0);
    out.texture = input.texture;
    out.light = input.light;
    return out;
}

@fragment
fn fragment(input: Out) -> @location(0) vec4<f32> {
    let colour = textureSample(surface, surface_sampler, input.texture);
    let light = textureSample(lightmap, lightmap_sampler, input.light).r;
    return vec4<f32>(colour.rgb * light, 1.0);
}
