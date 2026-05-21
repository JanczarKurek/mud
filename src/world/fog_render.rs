//! Fullscreen fog-of-war overlay (Material2d).
//!
//! Mirrors the `darkness_overlay` architecture: one big quad that follows the
//! camera, fed each frame with a packed bitmask of "discovered" tiles in a
//! window around the player. The shader (`assets/shaders/fog_of_war.wgsl`)
//! either renders transparent (discovered) or a deep dark starry pattern
//! (undiscovered), at a z high enough to cover sprites, NPCs, remote players,
//! and even the darkness overlay.
//!
//! Presentation-only — registered on `WorldClientPlugin`, never on
//! `WorldServerPlugin`. Data source is `ClientGameState.discovered_tiles`,
//! which the projection ([`crate::game::projection`]) fills from
//! `GameEvent::DiscoveredTilesReplaced` / `TilesDiscovered` deltas.

use bevy::asset::Asset;
use bevy::math::primitives::Rectangle;
use bevy::math::{UVec4, Vec4};
use bevy::mesh::{Mesh, Mesh2d, MeshVertexBufferLayoutRef};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, MeshMaterial2d};

use crate::game::resources::ClientGameState;
use crate::world::WorldConfig;

const SHADER_PATH: &str = "shaders/fog_of_war.wgsl";

/// Z above the darkness overlay (999.0) but still inside Bevy 2D's default
/// far plane (1000.0). Fog should not be tinted by the day/night curve — an
/// unexplored tile looks the same in daylight and at night.
const OVERLAY_Z: f32 = 999.5;

/// Edge length (world units) of the fog quad. Same constraint as the darkness
/// overlay: must exceed the camera viewport at any zoom.
const QUAD_SIZE: f32 = 4000.0;

/// Half-width of the discovered-mask window around the player, in tiles. The
/// mask covers a `(2R+1) × (2R+1)` square; anything outside the window reads
/// as undiscovered in the shader. This must be large enough that the visible
/// viewport never extends past the window — otherwise discovered tiles near
/// the monitor edge re-fog as the camera moves. At `tile_size = 48`, radius
/// 48 covers a 97-tile-wide window ≈ 4650 world units half-extent, comfortably
/// larger than 4K monitors at the current 1:1 projection.
const WINDOW_RADIUS: i32 = 48;
const WINDOW_W: i32 = WINDOW_RADIUS * 2 + 1;
const WINDOW_H: i32 = WINDOW_RADIUS * 2 + 1;

/// 80 vec4<u32> = 10240 bits, covering a 97×97 window (9409 bits) with
/// headroom. Must match the WGSL constant of the same name.
const MASK_VEC4_COUNT: usize = 80;

/// GPU uniforms. Layout must match the WGSL `FogUniforms` struct.
#[derive(Clone, ShaderType)]
pub struct FogUniforms {
    pub tile_xform: Vec4,
    pub mask_origin: Vec4,
    pub mask: [UVec4; MASK_VEC4_COUNT],
}

impl Default for FogUniforms {
    fn default() -> Self {
        Self {
            tile_xform: Vec4::ZERO,
            mask_origin: Vec4::ZERO,
            mask: [UVec4::ZERO; MASK_VEC4_COUNT],
        }
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone)]
pub struct FogOfWarMaterial {
    #[uniform(0)]
    pub uniforms: FogUniforms,
}

impl Default for FogOfWarMaterial {
    fn default() -> Self {
        Self {
            uniforms: FogUniforms::default(),
        }
    }
}

impl Material2d for FogOfWarMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        _descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        Ok(())
    }
}

/// Marker on the single fullscreen fog entity.
#[derive(Component)]
pub struct FogOverlay;

pub fn setup_fog_overlay(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FogOfWarMaterial>>,
    existing: Query<Entity, With<FogOverlay>>,
) {
    if existing.iter().next().is_some() {
        return;
    }
    let mesh = meshes.add(Rectangle::new(QUAD_SIZE, QUAD_SIZE));
    let material = materials.add(FogOfWarMaterial::default());
    commands.spawn((
        FogOverlay,
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, OVERLAY_Z),
        Visibility::default(),
    ));
}

pub fn update_fog_overlay(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    camera_query: Query<&Transform, (With<bevy::prelude::Camera2d>, Without<FogOverlay>)>,
    mut overlay_query: Query<(&MeshMaterial2d<FogOfWarMaterial>, &mut Transform), With<FogOverlay>>,
    mut materials: ResMut<Assets<FogOfWarMaterial>>,
) {
    let _t = crate::diagnostics::SystemTimer::new("update_fog_overlay", 1.0);
    let Ok((material_handle, mut overlay_transform)) = overlay_query.single_mut() else {
        return;
    };
    let Some(player_pos) = client_state.player_position else {
        return;
    };

    let space_id = player_pos.space_id;
    let player_z = player_pos.tile_position.z;
    let window_x0 = player_pos.tile_position.x - WINDOW_RADIUS;
    let window_y0 = player_pos.tile_position.y - WINDOW_RADIUS;

    // Build the discovered-tile bitmask for the visible window at the player's
    // floor. v1 only fogs the player's current floor — upper/lower bands are
    // depth cues that read as "we can see them" even when the player hasn't
    // explored that exact column.
    let mut mask = [UVec4::ZERO; MASK_VEC4_COUNT];
    if let Some(set) = client_state.discovered_tiles.get(&space_id) {
        for &(tx, ty, tz) in set.iter() {
            if tz != player_z {
                continue;
            }
            let mx = tx - window_x0;
            let my = ty - window_y0;
            if mx < 0 || mx >= WINDOW_W || my < 0 || my >= WINDOW_H {
                continue;
            }
            let bit_index = (my * WINDOW_W + mx) as u32;
            let u32_index = bit_index / 32;
            let bit_in_u32 = bit_index % 32;
            let vec4_index = (u32_index / 4) as usize;
            let component = (u32_index % 4) as usize;
            if vec4_index >= MASK_VEC4_COUNT {
                continue;
            }
            let v = &mut mask[vec4_index];
            let cur = v[component];
            v[component] = cur | (1u32 << bit_in_u32);
        }
    }

    // Tile-to-world transform — same convention as the darkness overlay
    // (sprites at absolute world coords, origin at `-0.5 * tile_size`).
    let tile_size = world_config.tile_size;
    let origin_x = -0.5 * tile_size;
    let origin_y = -0.5 * tile_size;

    // Park the overlay at the camera so the quad always covers the viewport.
    if let Ok(camera_transform) = camera_query.single() {
        let new_overlay_pos = Vec3::new(
            camera_transform.translation.x,
            camera_transform.translation.y,
            OVERLAY_Z,
        );
        if overlay_transform.translation != new_overlay_pos {
            overlay_transform.translation = new_overlay_pos;
        }
    }

    let Some(material) = materials.get_mut(&material_handle.0) else {
        return;
    };
    material.uniforms.tile_xform = Vec4::new(origin_x, origin_y, tile_size, 0.0);
    material.uniforms.mask_origin = Vec4::new(
        window_x0 as f32,
        window_y0 as f32,
        WINDOW_W as f32,
        WINDOW_H as f32,
    );
    material.uniforms.mask = mask;
}
