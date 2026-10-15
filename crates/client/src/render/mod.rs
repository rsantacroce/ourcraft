//! Block rendering: the texture array, the voxel material and its shader.
//!
//! **Texture array.** Instead of packing all block textures into one big
//! image (an atlas), we stack them as layers of a texture array. Each vertex
//! says which layer it uses. Unlike an atlas, a layer can repeat across a
//! large face, which greedy meshing (M2) needs, and there's no colour
//! bleeding between neighbouring textures.

mod textures;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError,
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

pub struct VoxelRenderPlugin;

impl Plugin for VoxelRenderPlugin {
    fn build(&self, app: &mut App) {
        // Compile the shader into the binary: no loose asset files to ship,
        // which matters for Android and iOS packaging later.
        embedded_asset!(app, "voxel.wgsl");
        app.add_plugins(MaterialPlugin::<VoxelMaterial>::default())
            .add_systems(Startup, setup_block_textures);
    }
}

/// The material for all chunk meshes.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct VoxelMaterial {
    #[texture(0, dimension = "2d_array")]
    #[sampler(1)]
    pub textures: Handle<Image>,
}

impl Material for VoxelMaterial {
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
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        Ok(())
    }
}

/// `layers[block_id][face]` → texture-array layer. Built from the registry.
#[derive(Resource)]
pub struct BlockTextureLayers(pub Vec<[u32; 6]>);

/// The one shared chunk material.
#[derive(Resource)]
pub struct VoxelMaterialHandle(pub Handle<VoxelMaterial>);

/// Generate every texture the registered blocks use, stack them into a
/// texture array, and record which layer each block face uses.
pub fn setup_block_textures(
    mut commands: Commands,
    blocks: Res<Blocks>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
) {
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
    let face_layers: Vec<[u32; 6]> = blocks
        .0
        .iter()
        .map(|(_, def)| match &def.textures {
            Some(t) => Face::ALL.map(|f| layer_of(t.for_face(f))),
            None => [0; 6],
        })
        .collect();

    // Stack the textures vertically: layer i is rows i*16 .. (i+1)*16.
    let layer_count = names.len().max(1) as u32;
    let mut data = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize * names.len());
    for name in &names {
        data.extend_from_slice(&generate_texture(name));
    }
    if names.is_empty() {
        data.extend_from_slice(&generate_texture("missing"));
    }

    let mut image = Image::new(
        Extent3d {
            width: TEXTURE_SIZE,
            height: TEXTURE_SIZE,
            depth_or_array_layers: layer_count,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Always view it as an array, even with a single layer, to match the
    // shader's `texture_2d_array`.
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    // Nearest-neighbour filtering keeps the crisp pixel-art look, and
    // `Repeat` lets one texture tile across a merged face.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });

    info!(
        "Block texture array: {} layers ({})",
        layer_count,
        names.join(", ")
    );
    let material = materials.add(VoxelMaterial {
        textures: images.add(image),
    });
    commands.insert_resource(BlockTextureLayers(face_layers));
    commands.insert_resource(VoxelMaterialHandle(material));
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
    .with_inserted_indices(Indices::U32(data.indices))
}
