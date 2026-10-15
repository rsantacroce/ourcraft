// Shader for chunk meshes: samples the block texture array and applies the
// per-vertex brightness baked in by the mesher. No dynamic lights, which
// keeps it fast on tablets and gives a clean cartoon look.

#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_clip}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var block_textures: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var block_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) layer: u32,
    @location(3) shade: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Integers can't be blended between vertices, so pass it through "flat".
    @location(1) @interpolate(flat) layer: u32,
    @location(2) shade: f32,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = mesh_position_local_to_clip(
        get_world_from_local(v.instance_index),
        vec4<f32>(v.position, 1.0),
    );
    out.uv = v.uv;
    out.layer = v.layer;
    out.shade = v.shade;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(block_textures, block_sampler, in.uv, in.layer);
    return vec4<f32>(color.rgb * in.shade, 1.0);
}
