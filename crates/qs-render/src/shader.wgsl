// SPDX-License-Identifier: GPL-3.0-or-later

// The world's surfaces: lit (REND-1.7), turbulent (REND-2.2, REND-9.1) and sky (REND-2.3,
// REND-8.1); alias models (REND-4.8); sprites (REND-11.2); particles (PART-3.2); the fog over
// them all (REND-10.4); and the 2D layer: the view blend (VIEW-4.6), the status bar, the
// scoreboards and the centre print (HUD-1 to HUD-3).

// REND-2.2: the peak of `turbsin`, 8 sin(2πi/256), in `WinQuake/gl_warp_sin.h`.
// REND-13.9: the gamma that leaves the frame as it is.
const NO_GAMMA: f32 = 1.0;
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
// REND-8.2: `view.skybox.x` above this when the sky is a skybox.
const NO_SKYBOX: f32 = 0.0;
// REND-8.3: the skybox's layers, in `suf`'s order, which `skytexorder` picks by axis.
const SKY_RT: i32 = 0;
const SKY_BK: i32 = 1;
const SKY_LF: i32 = 2;
const SKY_FT: i32 = 3;
const SKY_UP: i32 = 4;
const SKY_DN: i32 = 5;
// REND-8.3: `Sky_EmitSkyBoxVertex`'s "convert from range [-1,1] to [0,1]", `(s+1)*0.5`.
const SKY_ST_OFFSET: f32 = 1.0;
const SKY_ST_SCALE: f32 = 0.5;
// REND-8.3: "avoid bilerp seam": `s * (w-1)/w + 0.5/w`, and `t = 1.0 - t`.
const SKY_TEXEL: f32 = 1.0;
const SKY_TEXEL_CENTRE: f32 = 0.5;
const SKY_T_FLIP: f32 = 1.0;
// REND-8.3: a component below this looks along its axis's negative side (`v[i] < 0`).
const SKY_NEGATIVE: f32 = 0.0;
// The skybox has no mipmaps.
const SKY_LEVEL: f32 = 0.0;
// Every surface is drawn opaque: the sky's alpha layer is blended in the shader (REND-2.3).
const OPAQUE: f32 = 1.0;
// The w of a position, which makes it a point the view matrix moves.
const POINT: f32 = 1.0;
// `glAlphaFunc(GL_GREATER, 0.666)` in `GL_Init`: a sprite's or a picture's texel is drawn
// only when its alpha is above this (REND-11.2).
const ALPHA_TEST: f32 = 0.666;
// The 2D layer's clip space runs from -1 to 1 across the screen (`glOrtho`, HUD-1).
const CLIP_SPAN: f32 = 2.0;
const CLIP_START: f32 = -1.0;
// A 2D vertex's clip depth: everything in the layer is at one depth, in front.
const FLAT: f32 = 0.0;

struct View {
    clip_from_world: mat4x4<f32>,
    // The eye's position, and the time in seconds.
    eye_time: vec4<f32>,
    // The fog's colour, and `GL_FOG_DENSITY`, the density over 64 (REND-10.4).
    fog: vec4<f32>,
    // The water alpha (REND-9.1), the fog's share of the sky (REND-8.1), and the 2D layer's
    // width and height in its own units (HUD-1).
    params: vec4<f32>,
    // 1 when the sky is a skybox, then its faces' width and height (REND-8.2).
    skybox: vec4<f32>,
    // What a lightmap texel is multiplied by: 2 with QuakeSpasm's overbright (REND-13.5), then
    // the gamma every fragment is raised to, 1 for none (REND-13.9).
    look: vec4<f32>,
};

// REND-13.9: QuakeSpasm's `GLSLGamma_GammaCorrect`, "`gl_FragColor = vec4(pow(frag.rgb,
// vec3(GammaValue)), 1.0);`", here on each fragment as it is written.
fn corrected(colour: vec3<f32>) -> vec3<f32> {
    if view.look.y == NO_GAMMA {
        return colour;
    }
    return pow(max(colour, vec3<f32>()), vec3<f32>(view.look.y));
}

// REND-10.4: `GL_EXP2`, `1 - exp (-((density / 64) * z)^2)` of the fragment's distance from
// the eye, mixing its colour toward the fog's; with no fog the mix is 0.
fn fogged(colour: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let z = distance(world, view.eye_time.xyz);
    let exponent = view.fog.w * z;
    let share = OPAQUE - exp(-(exponent * exponent));
    return mix(colour, view.fog.rgb, share);
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var lightmap: texture_2d<f32>;
@group(0) @binding(2) var lightmap_sampler: sampler;
// The skybox's faces in `suf`'s order: rt, bk, lf, ft, up, dn (REND-8.2).
@group(0) @binding(3) var skybox: texture_2d_array<f32>;
@group(1) @binding(0) var surface: texture_2d<f32>;
@group(1) @binding(1) var surface_sampler: sampler;
// The sky's alpha layer; for any other surface or skin, its glow, black when it has none
// (REND-13.4).
@group(1) @binding(2) var second: texture_2d<f32>;
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
    let light = textureSample(lightmap, lightmap_sampler, input.light).rgb * view.look.x;
    // REND-13.4: "result += texture2D(FullbrightTex, ...)", then "clamp(result, 0.0, 1.0)".
    let glow = textureSample(second, surface_sampler, input.texture).rgb;
    let lit = clamp(colour.rgb * light + glow, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(corrected(fogged(lit, input.world)), OPAQUE);
}

// REND-2.2: `(s + 8 sin(t*0.125 + realtime)) / 64`, and t the same way round; REND-9.1:
// blended over the scene with the water alpha.
@fragment
fn turbulent(input: Out) -> @location(0) vec4<f32> {
    let time = view.eye_time.w;
    let st = input.texture;
    let s = (st.x + TURB_AMPLITUDE * sin(st.y * TURB_FREQUENCY + time)) / TURB_TILE;
    let t = (st.y + TURB_AMPLITUDE * sin(st.x * TURB_FREQUENCY + time)) / TURB_TILE;
    let colour = textureSample(surface, surface_sampler, vec2<f32>(s, t)).rgb;
    return vec4<f32>(corrected(fogged(colour, input.world)), view.params.x);
}

// REND-2.3: the solid layer scrolling under the alpha layer, both seen from the eye, or
// REND-8.2: the skybox's face the eye looks through; REND-8.1: the fog's colour over it by
// its share of the sky.
@fragment
fn sky(input: Out) -> @location(0) vec4<f32> {
    if (view.skybox.x > NO_SKYBOX) {
        let colour = box_texel(input.world - view.eye_time.xyz);
        return vec4<f32>(corrected(mix(colour, view.fog.rgb, view.params.y)), OPAQUE);
    }
    var dir = input.world - view.eye_time.xyz;
    dir.z = dir.z * SKY_FLATTEN;
    let length = SKY_RADIUS / max(sqrt(dot(dir, dir)), SKY_MIN_DISTANCE);
    let flat = dir.xy * length;
    let time = view.eye_time.w;
    let solid = textureSample(surface, surface_sampler, sky_st(flat, time * SKY_SOLID_SPEED));
    let alpha = textureSample(second, surface_sampler, sky_st(flat, time * SKY_ALPHA_SPEED));
    let layers = mix(solid.rgb, alpha.rgb, alpha.a);
    return vec4<f32>(corrected(mix(layers, view.fog.rgb, view.params.y)), OPAQUE);
}

// REND-8.2: `Sky_ProjectPoly`'s face for the direction `v`, by its largest axis, and its
// `vec_to_st` coordinates from -1 to 1; then `Sky_EmitSkyBoxVertex`'s texture coordinates,
// moved half a texel in to "avoid bilerp seam", t upside down, from the face `skytexorder`
// gives.
fn box_texel(v: vec3<f32>) -> vec3<f32> {
    let av = abs(v);
    var layer = SKY_UP;
    var st = vec2<f32>(0.0);
    if (av.x > av.y && av.x > av.z) {
        if (v.x < SKY_NEGATIVE) {
            layer = SKY_LF;
            st = vec2<f32>(v.y, v.z) / -v.x;
        } else {
            layer = SKY_RT;
            st = vec2<f32>(-v.y, v.z) / v.x;
        }
    } else if (av.y > av.z && av.y > av.x) {
        if (v.y < SKY_NEGATIVE) {
            layer = SKY_FT;
            st = vec2<f32>(-v.x, v.z) / -v.y;
        } else {
            layer = SKY_BK;
            st = vec2<f32>(v.x, v.z) / v.y;
        }
    } else if (v.z < SKY_NEGATIVE) {
        layer = SKY_DN;
        st = vec2<f32>(-v.y, v.x) / -v.z;
    } else {
        layer = SKY_UP;
        st = vec2<f32>(-v.y, -v.x) / v.z;
    }
    let size = view.skybox.yz;
    let unit = (st + SKY_ST_OFFSET) * SKY_ST_SCALE;
    let inset = unit * (size - SKY_TEXEL) / size + SKY_TEXEL_CENTRE / size;
    let uv = vec2<f32>(inset.x, SKY_T_FLIP - inset.y);
    return textureSampleLevel(skybox, lightmap_sampler, uv, layer, SKY_LEVEL).rgb;
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
    @location(2) world: vec3<f32>,
};

@vertex
fn alias_vertex(input: AliasIn) -> AliasOut {
    var out: AliasOut;
    out.clip = view.clip_from_world * vec4<f32>(input.position, POINT);
    out.texture = input.texture;
    out.shade = input.shade;
    out.world = input.position;
    return out;
}

// REND-4.8: the skin's colour times the vertex's shade, `GL_MODULATE`.
@fragment
fn alias_fragment(input: AliasOut) -> @location(0) vec4<f32> {
    let colour = textureSample(surface, surface_sampler, input.texture);
    // REND-13.4: a skin's glow, black for one with none.
    let glow = textureSample(second, surface_sampler, input.texture).rgb;
    let lit = clamp(colour.rgb * input.shade + glow, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(corrected(fogged(lit, input.world)), OPAQUE);
}

struct SpriteIn {
    @location(0) position: vec3<f32>,
    @location(1) texture: vec2<f32>,
};

struct SpriteOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) texture: vec2<f32>,
    @location(1) world: vec3<f32>,
};

@vertex
fn sprite_vertex(input: SpriteIn) -> SpriteOut {
    var out: SpriteOut;
    out.clip = view.clip_from_world * vec4<f32>(input.position, POINT);
    out.texture = input.texture;
    out.world = input.position;
    return out;
}

// REND-11.2: the frame's texel, its transparent texels cut by the alpha test.
@fragment
fn sprite_fragment(input: SpriteOut) -> @location(0) vec4<f32> {
    let colour = textureSample(surface, surface_sampler, input.texture);
    if colour.a <= ALPHA_TEST {
        discard;
    }
    return vec4<f32>(corrected(fogged(colour.rgb, input.world)), OPAQUE);
}

struct ParticleIn {
    @location(0) position: vec3<f32>,
    @location(1) texture: vec2<f32>,
    @location(2) colour: vec4<f32>,
};

struct ParticleOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) texture: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn particle_vertex(input: ParticleIn) -> ParticleOut {
    var out: ParticleOut;
    out.clip = view.clip_from_world * vec4<f32>(input.position, POINT);
    out.texture = input.texture;
    out.colour = input.colour;
    out.world = input.position;
    return out;
}

// PART-3.2: the particle's colour through the particle texture, `GL_MODULATE`, blended.
@fragment
fn particle_fragment(input: ParticleOut) -> @location(0) vec4<f32> {
    let texel = textureSample(surface, surface_sampler, input.texture);
    return vec4<f32>(corrected(fogged(input.colour.rgb * texel.rgb, input.world)), input.colour.a * texel.a);
}

struct FlatIn {
    @location(0) position: vec2<f32>,
    @location(1) texture: vec2<f32>,
    @location(2) colour: vec4<f32>,
};

struct FlatOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) texture: vec2<f32>,
    @location(1) colour: vec4<f32>,
};

// HUD-1: `glOrtho (0, vid.width, vid.height, 0, ...)`: the layer's units from the top left.
@vertex
fn flat_vertex(input: FlatIn) -> FlatOut {
    var out: FlatOut;
    let x = CLIP_START + input.position.x / view.params.z * CLIP_SPAN;
    let y = -(CLIP_START + input.position.y / view.params.w * CLIP_SPAN);
    out.clip = vec4<f32>(x, y, FLAT, POINT);
    out.texture = input.texture;
    out.colour = input.colour;
    return out;
}

// A picture or a character: its texel times the colour, transparent texels cut by the alpha
// test `GL_Set2D` turns on (HUD-1).
@fragment
fn flat_fragment(input: FlatOut) -> @location(0) vec4<f32> {
    let texel = textureSample(surface, surface_sampler, input.texture) * input.colour;
    if texel.a <= ALPHA_TEST {
        discard;
    }
    return vec4<f32>(corrected(texel.rgb), texel.a);
}

// A blend over the screen: the view blend (VIEW-4.6) and `Draw_FadeScreen`, with no alpha
// test.
@fragment
fn blend_fragment(input: FlatOut) -> @location(0) vec4<f32> {
    let texel = textureSample(surface, surface_sampler, input.texture) * input.colour;
    return vec4<f32>(corrected(texel.rgb), texel.a);
}
