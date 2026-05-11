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
use crate::world::components::ViewPosition;
use crate::world::floors::VisibleFloorRange;
use crate::world::lighting::{
    convert_authored_keyframes, default_day_night_curve, evaluate_ambient_curve, srgb_u8_to_linear,
    AmbientKeyframeF32, LightSource,
};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::WorldConfig;

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
    camera_query: Query<&Transform, (With<bevy::prelude::Camera2d>, Without<DarknessOverlay>)>,
    light_query: Query<(&LightSource, &ViewPosition, &Transform), Without<DarknessOverlay>>,
    mut overlay_query: Query<
        (&MeshMaterial2d<DarknessOverlayMaterial>, &mut Transform),
        With<DarknessOverlay>,
    >,
    mut materials: ResMut<Assets<DarknessOverlayMaterial>>,
) {
    let _t = crate::diagnostics::SystemTimer::new("update_darkness_overlay", 1.0);
    // The 4000×4000 quad has to follow the camera since sprites are now
    // rendered at absolute world coords; otherwise the screen window could
    // fall outside the quad's bounds and the shader would stop covering it.
    let Ok((material_handle, mut overlay_transform)) = overlay_query.single_mut() else {
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
    let indoor_rgb = srgb_u8_to_linear(lighting_cfg.indoor_ambient);
    let indoor_alpha = brightness_to_alpha(&indoor_rgb);

    // Outdoor: drive by per-map curve (or engine default) when has_day_night;
    // otherwise constant from outdoor_ambient (caves, dungeons). Curve owns
    // both color and alpha — daylight keyframes with alpha=0 implicitly hide
    // all light sources, since the shader only subtracts from alpha.
    let (outdoor_color, outdoor_alpha) = if lighting_cfg.has_day_night {
        let default_curve = default_day_night_curve();
        let owned: Vec<AmbientKeyframeF32>;
        let curve: &[AmbientKeyframeF32] = if lighting_cfg.outdoor_curve.is_empty() {
            &default_curve
        } else {
            owned = convert_authored_keyframes(&lighting_cfg.outdoor_curve);
            &owned
        };
        evaluate_ambient_curve(curve, client_state.world_time)
    } else {
        let rgb = srgb_u8_to_linear(lighting_cfg.outdoor_ambient);
        let a = brightness_to_alpha(&rgb);
        (rgb, a)
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
    // Sprites now sit at absolute world coords (`tile * tile_size`), so the
    // shader maps a fragment's world_xy to a tile via
    // `floor((world_xy - origin) / tile_size)` with origin = `-0.5 * tile_size`.
    // The −0.5 offset places tile boundaries between sprite centers (sprite
    // for tile T centered at T*tile_size occupies
    // [(T-0.5)*tile_size, (T+0.5)*tile_size)). Origin is *constant* now —
    // player position and scroll dropped out when we switched to a
    // camera-follow scheme.
    let tile_size = world_config.tile_size;
    let origin_x = -0.5 * tile_size;
    let origin_y = -0.5 * tile_size;

    // Park the overlay quad at the camera's world position so it covers the
    // visible viewport regardless of where the camera has scrolled to. Cheap:
    // single Transform write per camera move.
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
    material.uniforms.outdoor = Vec4::new(
        outdoor_color[0],
        outdoor_color[1],
        outdoor_color[2],
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

/// Map an ambient color (0..1 RGB) to a darkness alpha for the
/// non-curve-driven paths (indoor; outdoor when `has_day_night: false`).
/// Bright ambient ⇒ low alpha (transparent overlay); dark ambient ⇒ high
/// alpha. Capped at 0.95 so pure-black ambient still leaves a sliver of
/// visibility for sprites under the overlay.
fn brightness_to_alpha(rgb: &[f32; 3]) -> f32 {
    let brightness = rgb[0].max(rgb[1]).max(rgb[2]);
    (1.0 - brightness).clamp(0.0, 0.95)
}
