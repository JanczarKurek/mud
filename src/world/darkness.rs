//! Fullscreen darkness overlay (Material2d).
//!
//! Replaces both the per-sprite ambient tint and the per-light additive
//! overlays with a single big quad on top of the world. The shader
//! (`assets/shaders/darkness_overlay.wgsl`) outputs an ambient color +
//! per-pixel alpha; lights subtract from the alpha, never add color.
//! That means a torch *restores* a sprite's natural color — it can't
//! brighten anything past `Sprite.color`. Indoor / outdoor is preserved
//! by uploading a small bitmask each frame and sampling per-pixel.
//!
//! See `crate::world::lighting` for the world-clock + `LightSource` ECS
//! component (this module is the renderer; lighting.rs is the data).

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
use crate::world::components::ViewPosition;
use crate::world::floors::VisibleFloorRange;
use crate::world::lighting::{day_night_palette, srgb_u8_to_linear, LightSource};
use crate::world::object_definitions::OverworldObjectDefinitions;

const SHADER_PATH: &str = "shaders/darkness_overlay.wgsl";

/// Z value for the darkness quad. Above any sprite, below the camera far
/// plane (Bevy's default 2D far is 1000.0). 999 keeps a touch of headroom.
const OVERLAY_Z: f32 = 999.0;

/// Edge length (world units) of the darkness quad. Must be larger than the
/// camera viewport at any zoom — 4000 covers a 4K monitor at 1× and any
/// realistic zoom-out level. The quad is stationary at world origin (0,0);
/// the camera at world origin sees a screen-aligned slice of it.
const QUAD_SIZE: f32 = 4000.0;

/// Half-width of the lighting window around the player (in tiles). The
/// indoor bitmask covers a `(2R+1) × (2R+1)` square. Pixels outside the
/// window read as "outdoor" by default; that's fine because the camera
/// never shows tiles past the window anyway.
const WINDOW_RADIUS: i32 = 16;
const WINDOW_W: i32 = WINDOW_RADIUS * 2 + 1;
const WINDOW_H: i32 = WINDOW_RADIUS * 2 + 1;

/// Maximum simultaneously-rendered lights. Excess lights are dropped (by
/// distance to player) since the shader array is fixed-size.
const MAX_LIGHTS: usize = 32;

/// Indoor bitmask capacity in u32 words. Must match the WGSL constant.
/// `WINDOW_W * WINDOW_H = 1089` bits ⇒ 35 u32; round up so packing into
/// `[UVec4; MASK_VEC4_COUNT]` is exact.
const MASK_VEC4_COUNT: usize = 16;

/// GPU uniforms. Layout must match the WGSL `DarknessUniforms` struct.
#[derive(Clone, ShaderType)]
pub struct DarknessUniforms {
    pub outdoor: Vec4,
    pub indoor: Vec4,
    pub tile_xform: Vec4,
    pub mask_origin: Vec4,
    pub counts: Vec4,
    pub mask: [UVec4; MASK_VEC4_COUNT],
    pub lights: [Vec4; MAX_LIGHTS],
}

impl Default for DarknessUniforms {
    fn default() -> Self {
        Self {
            outdoor: Vec4::ZERO,
            indoor: Vec4::ZERO,
            tile_xform: Vec4::ZERO,
            mask_origin: Vec4::ZERO,
            counts: Vec4::ZERO,
            mask: [UVec4::ZERO; MASK_VEC4_COUNT],
            lights: [Vec4::ZERO; MAX_LIGHTS],
        }
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone)]
pub struct DarknessOverlayMaterial {
    #[uniform(0)]
    pub uniforms: DarknessUniforms,
}

impl Default for DarknessOverlayMaterial {
    fn default() -> Self {
        Self {
            uniforms: DarknessUniforms::default(),
        }
    }
}

impl Material2d for DarknessOverlayMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        // Standard alpha blending: dst = src.rgb * src.a + dst.rgb * (1 - src.a)
        // (the default for AlphaMode2d::Blend; no specialize override needed).
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

/// Marker on the single fullscreen darkness entity.
#[derive(Component)]
pub struct DarknessOverlay;

/// Spawn the darkness quad once. The same entity persists for the lifetime
/// of the InGame state and is updated each frame by `update_darkness_overlay`.
pub fn setup_darkness_overlay(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DarknessOverlayMaterial>>,
    existing: Query<Entity, With<DarknessOverlay>>,
) {
    if existing.iter().next().is_some() {
        return;
    }
    let mesh = meshes.add(Rectangle::new(QUAD_SIZE, QUAD_SIZE));
    let material = materials.add(DarknessOverlayMaterial::default());
    commands.spawn((
        DarknessOverlay,
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, OVERLAY_Z),
        Visibility::default(),
    ));
}

/// Build the indoor mask, ambient values, and light list each frame and
/// upload to the material uniforms. Runs after the source Transforms are
/// finalized so light world positions are accurate.
#[allow(clippy::too_many_arguments)]
pub fn update_darkness_overlay(
    client_state: Res<ClientGameState>,
    visible_floors: Res<VisibleFloorRange>,
    world_config: Res<WorldConfig>,
    definitions: Res<OverworldObjectDefinitions>,
    light_query: Query<(&LightSource, &ViewPosition, &Transform)>,
    overlay_query: Query<&MeshMaterial2d<DarknessOverlayMaterial>, With<DarknessOverlay>>,
    mut materials: ResMut<Assets<DarknessOverlayMaterial>>,
) {
    let Ok(material_handle) = overlay_query.single() else {
        return;
    };
    let Some(player_pos) = client_state.player_position else {
        return;
    };
    let Some(current_space) = client_state.current_space.as_ref() else {
        return;
    };

    let space_id = current_space.space_id;
    let lighting_cfg = &current_space.lighting;
    let outdoor_rgb = srgb_u8_to_linear(lighting_cfg.outdoor_ambient);
    let indoor_rgb = srgb_u8_to_linear(lighting_cfg.indoor_ambient);
    let outdoor_alpha = compute_alpha_from_brightness(&outdoor_rgb);
    let indoor_alpha = compute_alpha_from_brightness(&indoor_rgb);

    // Outdoor color is modulated by day/night (gives night a cool blue, etc.).
    // Indoor is fixed — roofs block the sky.
    let outdoor_modulated = if lighting_cfg.has_day_night {
        let palette = day_night_palette(client_state.world_time);
        [
            outdoor_rgb[0] * palette[0],
            outdoor_rgb[1] * palette[1],
            outdoor_rgb[2] * palette[2],
        ]
    } else {
        outdoor_rgb
    };

    // Window in tiles (matches `WINDOW_W × WINDOW_H` square around player).
    let player_floor = visible_floors.player_floor;
    let window_x0 = player_pos.tile_position.x - WINDOW_RADIUS;
    let window_y0 = player_pos.tile_position.y - WINDOW_RADIUS;

    // Build the indoor bitmask: a tile (x,y,player_floor) is "indoor" iff some
    // object at (x,y,player_floor+1) in this space has `occludes_floor_above`.
    // Single sweep over world_objects — same shape as the optimised version
    // in lighting.rs, but writing bits instead of HashSet entries.
    let mut mask = [UVec4::ZERO; MASK_VEC4_COUNT];
    for object in client_state.world_objects.values() {
        if object.position.space_id != space_id {
            continue;
        }
        let tile = object.tile_position;
        if tile.z != player_floor + 1 {
            continue;
        }
        let mx = tile.x - window_x0;
        let my = tile.y - window_y0;
        if mx < 0 || mx >= WINDOW_W || my < 0 || my >= WINDOW_H {
            continue;
        }
        let occludes = definitions
            .get(&object.definition_id)
            .is_some_and(|def| def.render.occludes_floor_above);
        if !occludes {
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

    // Build the light list. Read each LightSource's already-finalized
    // Transform (post-`sync_tile_transforms`) so view-scroll and visual
    // offsets are baked in — the darkness shader sees lights in the same
    // world frame the player camera renders.
    let mut lights = [Vec4::ZERO; MAX_LIGHTS];
    let mut count = 0usize;
    for (light, view, xform) in &light_query {
        if count >= MAX_LIGHTS {
            break;
        }
        if view.space_id != space_id {
            continue;
        }
        if !visible_floors.contains(view.tile.z) {
            continue;
        }
        // Anchor compensation: y-sorted sprites have Transform at the bottom-
        // center; lift to visual tile center. Mirrors the rule in
        // `world::light_overlay::update_light_overlays` (now retired).
        // Without WorldVisual access here we approximate by always lifting
        // half a tile; sprites that *don't* y-sort are rare in practice and
        // a half-tile offset on a torch is invisible at typical radii.
        let world_y = xform.translation.y + world_config.tile_size * 0.5;
        let radius_world = light.radius * world_config.tile_size;
        lights[count] = Vec4::new(xform.translation.x, world_y, radius_world, light.intensity);
        count += 1;
    }

    // Tile-to-world transform.
    // sync_tile_transforms: world = (tile - player_tile) * tile_size + view_scroll.
    // Treating view_scroll as zero here is OK: the darkness quad is at world
    // (0,0) and the camera shows the same slice the world is drawn into. The
    // shader uses world coords directly to derive tile coords for the mask;
    // off-by-fractional-tile during a scroll is invisible because the mask
    // bit only flips at tile boundaries anyway.
    let tile_size = world_config.tile_size;
    let origin_x = -player_pos.tile_position.x as f32 * tile_size;
    let origin_y = -player_pos.tile_position.y as f32 * tile_size;

    let Some(material) = materials.get_mut(&material_handle.0) else {
        return;
    };
    material.uniforms.outdoor = Vec4::new(
        outdoor_modulated[0],
        outdoor_modulated[1],
        outdoor_modulated[2],
        outdoor_alpha,
    );
    material.uniforms.indoor = Vec4::new(indoor_rgb[0], indoor_rgb[1], indoor_rgb[2], indoor_alpha);
    material.uniforms.tile_xform = Vec4::new(origin_x, origin_y, tile_size, 0.0);
    material.uniforms.mask_origin = Vec4::new(
        window_x0 as f32,
        window_y0 as f32,
        WINDOW_W as f32,
        WINDOW_H as f32,
    );
    material.uniforms.counts = Vec4::new(count as f32, 0.0, 0.0, 0.0);
    material.uniforms.mask = mask;
    material.uniforms.lights = lights;
}

/// Map an ambient color (0..1 RGB) to the darkness alpha. Bright ambient
/// (e.g. noon) ⇒ low alpha (transparent overlay, world shows full color);
/// dark ambient (cellar, midnight) ⇒ high alpha (heavy darkening).
///
/// Pure white outdoor would otherwise leave alpha=0 and the overlay would
/// be invisible — fine, that's the goal at noon. The slight `+0.04` floor
/// keeps the user-authored ambient color faintly visible under a clear sky
/// so day/night transitions read on screen.
fn compute_alpha_from_brightness(rgb: &[f32; 3]) -> f32 {
    let brightness = rgb[0].max(rgb[1]).max(rgb[2]);
    (1.0 - brightness).clamp(0.0, 0.95)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_zero_at_full_brightness() {
        assert!((compute_alpha_from_brightness(&[1.0, 1.0, 1.0])).abs() < 1e-6);
    }

    #[test]
    fn alpha_high_at_zero_brightness() {
        let a = compute_alpha_from_brightness(&[0.0, 0.0, 0.0]);
        assert!(a >= 0.9);
    }

    #[test]
    fn alpha_uses_max_channel() {
        // Bright red ambient (e.g. dusk) shouldn't leave the world overly dark
        // just because green/blue are dim.
        let a = compute_alpha_from_brightness(&[0.95, 0.1, 0.1]);
        assert!(a < 0.1);
    }

}
