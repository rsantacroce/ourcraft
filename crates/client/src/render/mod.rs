//! Block rendering: the texture array, the voxel material and its shader.
//!
//! **Texture array.** Instead of packing all block textures into one big
//! image (an atlas), we stack them as layers of a texture array. Each vertex
//! says which layer it uses. Unlike an atlas, a layer can repeat across a
//! large face, which greedy meshing needs, and there's no colour bleeding
//! between neighbouring textures.
//!
//! **Mipmaps.** Far-away blocks cover only a few pixels on screen, so
//! sampling a 16×16 texture there gives shimmering noise. Mipmaps are
//! pre-shrunk copies (8×8, 4×4, …) the GPU picks from automatically.
//!
//! **Fog.** Chunks only exist up to the render distance. Fading distant
//! terrain into the sky colour hides that edge (and chunks popping in).

mod adventure_icons;
mod colour_textures;
mod stone_textures;
mod wood_textures;
pub use wood_textures::wood_colour;
mod farm_icons;
mod farm_textures;
pub mod item_icons;
pub mod textures;

use std::sync::Arc;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension, VertexFormat,
};
use bevy::shader::ShaderRef;

use crate::meshing::ChunkMeshData;
use crate::world::Blocks;
use textures::{TEXTURE_SIZE, generate_texture};

const SHADER_PATH: &str = "embedded://ourcraft_client/render/voxel.wgsl";

/// Custom vertex attributes. The ids just need to be unique and large.
pub const ATTRIBUTE_TEXTURE_LAYER: MeshVertexAttribute =
    MeshVertexAttribute::new("TextureLayer", 917_405_311, VertexFormat::Uint32);
pub const ATTRIBUTE_SHADE: MeshVertexAttribute =
    MeshVertexAttribute::new("Shade", 917_405_312, VertexFormat::Float32);
/// Sky light and block light, each 0..1.
pub const ATTRIBUTE_LIGHT: MeshVertexAttribute =
    MeshVertexAttribute::new("Light", 917_405_313, VertexFormat::Float32x2);

pub struct VoxelRenderPlugin;

impl Plugin for VoxelRenderPlugin {
    fn build(&self, app: &mut App) {
        // Compile the shader into the binary: no loose asset files to ship,
        // which matters for Android and iOS packaging.
        embedded_asset!(app, "voxel.wgsl");
        app.add_plugins(MaterialPlugin::<VoxelMaterial>::default())
            .init_resource::<ModTextures>()
            .add_systems(Startup, setup_block_textures)
            .add_systems(
                PreUpdate,
                apply_content_pack.after(crate::net::NetReceiveSet),
            );
    }
}

/// The material for all chunk meshes.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct VoxelMaterial {
    #[texture(0, dimension = "2d_array")]
    #[sampler(1)]
    pub textures: Handle<Image>,
    #[uniform(2)]
    pub params: VoxelParams,
    /// `Opaque` for solid blocks (cut-out pixels are discarded in the
    /// shader), `Blend` for water.
    pub alpha_mode: AlphaMode,
}

/// Values the shader reads that change at runtime (fog, time of day).
#[derive(ShaderType, Debug, Clone, Copy)]
pub struct VoxelParams {
    /// Colour distant terrain fades into (the sky colour), linear RGBA.
    pub fog_color: Vec4,
    /// Horizontal distance where fog starts, in blocks.
    pub fog_start: f32,
    /// Distance where terrain is fully hidden by fog.
    pub fog_end: f32,
    /// Sunlight multiplier: 1 at noon, low at night.
    pub daylight: f32,
    /// Darkest the world gets, so night is dim but still playable.
    pub min_light: f32,
}

impl Default for VoxelParams {
    fn default() -> Self {
        Self {
            fog_color: Vec4::new(0.25, 0.52, 0.9, 1.0),
            fog_start: 60.0,
            fog_end: 120.0,
            daylight: 1.0,
            min_light: 0.12,
        }
    }
}

impl Material for VoxelMaterial {
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn vertex_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    // Our lighting is baked into vertices, so skip Bevy's depth prepass and
    // shadow passes. Fewer passes also means better performance on tablets.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// Tell the GPU pipeline which mesh attributes go to which shader inputs.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            ATTRIBUTE_TEXTURE_LAYER.at_shader_location(2),
            ATTRIBUTE_SHADE.at_shader_location(3),
            ATTRIBUTE_LIGHT.at_shader_location(4),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        Ok(())
    }
}

/// Textures sent by the server for mods' blocks: name → 16×16 RGBA.
#[derive(Resource, Default, Clone)]
pub struct ModTextures(pub Arc<std::collections::HashMap<String, Vec<u8>>>);

impl ModTextures {
    /// Pixels for a texture name: a mod's texture, else a built-in one.
    pub fn pixels(&self, name: &str) -> Vec<u8> {
        self.0
            .get(name)
            .cloned()
            .unwrap_or_else(|| generate_texture(name))
    }
}

/// When we join a server, it sends the full list of blocks (including ones
/// added by mods) and the mods' textures. Rebuild the registry and the
/// texture array to match.
fn apply_content_pack(
    mut commands: Commands,
    mut messages: MessageReader<crate::net::FromServer>,
    handles: Option<Res<VoxelMaterialHandle>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
) {
    use ourcraft_common::block::BlockRegistry;
    use ourcraft_common::protocol::ServerMessage;
    for crate::net::FromServer(msg) in messages.read() {
        let ServerMessage::Welcome { content, .. } = msg else {
            continue;
        };
        let Some(registry) = BlockRegistry::from_defs(content.blocks.clone()) else {
            warn!("Server sent an invalid block list; keeping the built-in blocks");
            continue;
        };
        let textures = ModTextures(Arc::new(content.textures.iter().cloned().collect()));
        let (image, face_layers, names) = build_texture_array(&registry, |n| textures.pixels(n));
        info!(
            "Server content: {} blocks, {} textures ({} from mods)",
            registry.len(),
            names.len(),
            content.textures.len()
        );
        let image = images.add(image);
        if let Some(handles) = &handles {
            for h in handles.both() {
                if let Some(mut m) = materials.get_mut(h) {
                    m.textures = image.clone();
                }
            }
        }
        commands.insert_resource(BlockTextureLayers(Arc::new(face_layers)));
        commands.insert_resource(crate::world::Blocks(Arc::new(registry)));
        commands.insert_resource(textures);
    }
}

/// `layers[block_id][face]` → texture-array layer. Built from the registry.
/// In an `Arc` so meshing tasks on other threads can share it.
#[derive(Resource, Clone)]
pub struct BlockTextureLayers(pub Arc<Vec<[u32; 6]>>);

/// The two shared chunk materials.
#[derive(Resource)]
pub struct VoxelMaterialHandle {
    pub solid: Handle<VoxelMaterial>,
    pub translucent: Handle<VoxelMaterial>,
}

impl VoxelMaterialHandle {
    pub fn both(&self) -> [&Handle<VoxelMaterial>; 2] {
        [&self.solid, &self.translucent]
    }
}

/// Generate every texture the registered blocks use, stack them into a
/// texture array, and record which layer each block face uses.
pub fn setup_block_textures(
    mut commands: Commands,
    blocks: Res<Blocks>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
) {
    let (image, face_layers, names) = build_texture_array(&blocks.0, generate_texture);
    info!(
        "Block texture array: {} layers ({})",
        names.len(),
        names.join(", ")
    );
    let textures = images.add(image);
    let mut material = |alpha_mode| {
        materials.add(VoxelMaterial {
            textures: textures.clone(),
            params: VoxelParams::default(),
            alpha_mode,
        })
    };
    let handles = VoxelMaterialHandle {
        solid: material(AlphaMode::Opaque),
        translucent: material(AlphaMode::Blend),
    };
    commands.insert_resource(BlockTextureLayers(Arc::new(face_layers)));
    commands.insert_resource(handles);
}

/// Build the texture array image for all textures used by `registry`.
/// `pixels(name)` returns the 16×16 RGBA pixels of a named texture.
/// Returns the image, the per-block face layers and the layer names.
pub fn build_texture_array(
    registry: &ourcraft_common::block::BlockRegistry,
    pixels: impl Fn(&str) -> Vec<u8>,
) -> (Image, Vec<[u32; 6]>, Vec<String>) {
    use ourcraft_common::block::Face;

    // Assign a layer to each distinct texture name, in first-seen order.
    let mut names: Vec<String> = Vec::new();
    let mut layer_of = |name: &str| -> u32 {
        match names.iter().position(|n| n == name) {
            Some(i) => i as u32,
            None => {
                names.push(name.to_owned());
                (names.len() - 1) as u32
            }
        }
    };
    let face_layers: Vec<[u32; 6]> = registry
        .iter()
        .map(|(_, def)| match &def.textures {
            Some(t) => Face::ALL.map(|f| layer_of(t.for_face(f))),
            None => [0; 6],
        })
        .collect();
    if names.is_empty() {
        names.push("missing".to_owned());
    }
    let layer_count = names.len() as u32;

    // Data order: for each layer, every mip level from largest to smallest.
    let mut data = Vec::new();
    for name in &names {
        let mut level = pixels(name);
        let mut size = TEXTURE_SIZE;
        data.extend_from_slice(&level);
        while size > 1 {
            level = downsample(&level, size);
            size /= 2;
            data.extend_from_slice(&level);
        }
    }

    let mut image = Image::new_uninit(
        Extent3d {
            width: TEXTURE_SIZE,
            height: TEXTURE_SIZE,
            depth_or_array_layers: layer_count,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = TEXTURE_SIZE.ilog2() + 1;
    // Always view it as an array, even with a single layer, to match the
    // shader's `texture_2d_array`.
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    // Nearest-neighbour magnification keeps the crisp pixel-art look up
    // close; blending between mip levels smooths things out far away.
    // `Repeat` lets one texture tile across a merged face.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    (image, face_layers, names)
}

/// Halve an RGBA image (`size`×`size` → half that) by averaging 2×2 blocks.
/// Colours are weighted by alpha so see-through pixels don't darken edges.
fn downsample(src: &[u8], size: u32) -> Vec<u8> {
    let (s, half) = (size as usize, (size / 2) as usize);
    let mut out = Vec::with_capacity(half * half * 4);
    for y in 0..half {
        for x in 0..half {
            let mut rgb = [0u32; 3];
            let mut alpha = 0u32;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = ((y * 2 + dy) * s + x * 2 + dx) * 4;
                let a = src[i + 3] as u32;
                for (c, sum) in rgb.iter_mut().enumerate() {
                    *sum += src[i + c] as u32 * a;
                }
                alpha += a;
            }
            for c in rgb {
                out.push(c.checked_div(alpha).unwrap_or(0) as u8);
            }
            out.push((alpha / 4) as u8);
        }
    }
    out
}

/// Convert mesher output into a Bevy mesh.
pub fn build_mesh(data: ChunkMeshData) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        // The CPU copy isn't needed after upload; dropping it saves memory.
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs)
    .with_inserted_attribute(ATTRIBUTE_TEXTURE_LAYER, data.layers)
    .with_inserted_attribute(ATTRIBUTE_SHADE, data.shades)
    .with_inserted_attribute(ATTRIBUTE_LIGHT, data.lights)
    .with_inserted_indices(Indices::U32(data.indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_averages_and_respects_alpha() {
        // 2×2 image: two opaque white pixels, two fully transparent black.
        let src = [
            255, 255, 255, 255, 0, 0, 0, 0, //
            0, 0, 0, 0, 255, 255, 255, 255,
        ];
        let out = downsample(&src, 2);
        // Colour stays white (transparent pixels don't darken it); alpha halves.
        assert_eq!(out, vec![255, 255, 255, 127]);
    }
}
