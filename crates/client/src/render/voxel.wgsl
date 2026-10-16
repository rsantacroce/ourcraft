// Shader for chunk meshes: samples the block texture array, applies the
// per-vertex brightness baked in by the mesher (face direction + ambient
// occlusion), the light levels (sky light, dimmed at night, and warm block
// light from torches and lamps), and distance fog. No dynamic lights, which
// keeps it fast on tablets and gives a clean cartoon look.

#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_world}
#import bevy_pbr::view_transformations::position_world_to_clip
#import bevy_pbr::mesh_view_bindings::view

struct VoxelParams {
    fog_color: vec4<f32>,
    fog_start: f32,
    fog_end: f32,
    daylight: f32,
    min_light: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var block_textures: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var block_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> params: VoxelParams;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) layer: u32,
    @location(3) shade: f32,
    @location(4) light: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Integers can't be blended between vertices, so pass it through "flat".
    @location(1) @interpolate(flat) layer: u32,
    @location(2) shade: f32,
    @location(3) world_position: vec3<f32>,
    @location(4) light: vec2<f32>,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world = mesh_position_local_to_world(
        get_world_from_local(v.instance_index),
        vec4<f32>(v.position, 1.0),
    );
    out.clip_position = position_world_to_clip(world.xyz);
    out.world_position = world.xyz;
    out.uv = v.uv;
    out.layer = v.layer;
    out.shade = v.shade;
    out.light = v.light;
    return out;
}

// Light level 0..1 -> brightness. Level 15 is full brightness, and each step
// down is about 18% darker.
fn level(l: f32) -> f32 {
    return pow(0.82, (1.0 - l) * 15.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(block_textures, block_sampler, in.uv, in.layer);
    // Cut-out blocks (leaves, glass) have fully see-through pixels. Water's
    // pixels are partly see-through and get blended (in a separate pass).
    if color.a < 0.1 {
        discard;
    }

    var light: vec3<f32>;
    if in.shade > 1.5 {
        // Glowing block: always fully bright.
        light = vec3<f32>(1.0);
    } else {
        // Each light level is a bit darker than the one above it.
        let sky = level(in.light.x) * max(params.daylight, params.min_light);
        // Torches and lamps give a warm light.
        let glow = level(in.light.y) * vec3<f32>(1.0, 0.88, 0.7);
        // Never pitch black, even deep in a cave (0.06 normally; a glow
        // potion raises `min_light`, which lights caves up too).
        let darkest = params.min_light * params.min_light * 0.67;
        light = max(max(vec3<f32>(sky), glow), vec3<f32>(darkest)) * in.shade;
    }
    let lit = color.rgb * light;

    // Fog by horizontal distance, so looking up or down doesn't change it.
    let dist = distance(in.world_position.xz, view.world_position.xz);
    let fog = smoothstep(params.fog_start, params.fog_end, dist);
    return vec4<f32>(mix(lit, params.fog_color.rgb, fog), color.a);
}
