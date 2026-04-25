use std::collections::HashMap;

use bevy::prelude::*;

use crate::floor_viewer::render::{rebuild_render_cells, GRID_H, GRID_W, TILE_SIZE};
use crate::floor_viewer::ui::{
    spawn_palette_ui, sync_palette_highlight, sync_palette_panel, sync_status_text, PaletteDirty,
};
use crate::world::floor_definitions::{FloorTilesetDefinitions, FloorTypeId};
use crate::world::floor_map::FloorMap;

#[derive(Component)]
pub struct ViewerCamera;

#[derive(Resource)]
pub struct ViewerFloorMap(pub FloorMap);

impl Default for ViewerFloorMap {
    fn default() -> Self {
        Self(FloorMap::new_filled(GRID_W, GRID_H, None))
    }
}

#[derive(Resource, Default, Debug, Clone)]
pub struct ActiveFloor(pub Option<FloorTypeId>);

#[derive(Resource, Default)]
pub struct FloorTilesetAtlases {
    pub layouts: HashMap<FloorTypeId, Handle<TextureAtlasLayout>>,
    pub images: HashMap<FloorTypeId, Handle<Image>>,
}

#[derive(Resource)]
pub struct ViewerDirty(pub bool);

impl Default for ViewerDirty {
    fn default() -> Self {
        Self(true)
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewModeKind {
    #[default]
    Tiled,
    Debug,
}

#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct ViewMode(pub ViewModeKind);

#[derive(Resource, Clone, Copy, Debug)]
pub struct ShowGrid(pub bool);

impl Default for ShowGrid {
    fn default() -> Self {
        Self(true)
    }
}

pub struct FloorViewerPlugin;

impl Plugin for FloorViewerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FloorTilesetDefinitions::load_from_disk())
            .init_resource::<ViewerFloorMap>()
            .init_resource::<ActiveFloor>()
            .init_resource::<FloorTilesetAtlases>()
            .init_resource::<ViewerDirty>()
            .init_resource::<ViewMode>()
            .init_resource::<ShowGrid>()
            .init_resource::<PaletteDirty>()
            .add_systems(Startup, (setup_camera, spawn_palette_ui).chain())
            .add_systems(
                Update,
                (
                    palette_input,
                    reload_input,
                    toggle_view_input,
                    paint_input,
                    rebuild_render_cells,
                    sync_palette_panel,
                    sync_palette_highlight,
                    sync_status_text,
                    draw_grid_overlay,
                )
                    .chain(),
            );
    }
}

fn toggle_view_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut view: ResMut<ViewMode>,
    mut grid: ResMut<ShowGrid>,
    mut dirty: ResMut<ViewerDirty>,
) {
    if keys.just_pressed(KeyCode::KeyT) {
        view.0 = match view.0 {
            ViewModeKind::Tiled => ViewModeKind::Debug,
            ViewModeKind::Debug => ViewModeKind::Tiled,
        };
        dirty.0 = true;
    }
    if keys.just_pressed(KeyCode::KeyG) {
        grid.0 = !grid.0;
    }
}

fn draw_grid_overlay(mut gizmos: Gizmos, show: Res<ShowGrid>) {
    if !show.0 {
        return;
    }
    let half_w = GRID_W as f32 / 2.0 * TILE_SIZE;
    let half_h = GRID_H as f32 / 2.0 * TILE_SIZE;
    let inner = Color::srgba(1.0, 1.0, 1.0, 0.18);
    let outer = Color::srgba(1.0, 0.85, 0.25, 0.65);

    for x in 0..=GRID_W {
        let wx = (x as f32 - GRID_W as f32 / 2.0) * TILE_SIZE;
        let c = if x == 0 || x == GRID_W { outer } else { inner };
        gizmos.line_2d(Vec2::new(wx, -half_h), Vec2::new(wx, half_h), c);
    }
    for y in 0..=GRID_H {
        let wy = (y as f32 - GRID_H as f32 / 2.0) * TILE_SIZE;
        let c = if y == 0 || y == GRID_H { outer } else { inner };
        gizmos.line_2d(Vec2::new(-half_w, wy), Vec2::new(half_w, wy), c);
    }
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((Camera2d, ViewerCamera));
}

fn palette_input(
    keys: Res<ButtonInput<KeyCode>>,
    floor_defs: Res<FloorTilesetDefinitions>,
    mut active: ResMut<ActiveFloor>,
) {
    if keys.just_pressed(KeyCode::Digit0) {
        active.0 = None;
        return;
    }
    let mut ids: Vec<&str> = floor_defs.ids().collect();
    ids.sort();
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (i, key) in digits.iter().enumerate() {
        if keys.just_pressed(*key) {
            if let Some(id) = ids.get(i) {
                active.0 = Some((*id).to_owned());
            }
            return;
        }
    }
}

fn reload_input(
    keys: Res<ButtonInput<KeyCode>>,
    asset_server: Res<AssetServer>,
    mut floor_defs: ResMut<FloorTilesetDefinitions>,
    mut atlases: ResMut<FloorTilesetAtlases>,
    mut dirty: ResMut<ViewerDirty>,
    mut palette_dirty: ResMut<PaletteDirty>,
) {
    if !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    info!("reloading floor tileset definitions from disk");
    let new_defs = FloorTilesetDefinitions::load_from_disk();
    let atlas_paths: Vec<String> = new_defs
        .iter()
        .filter_map(|def| def.atlas_path.clone())
        .collect();
    for p in atlas_paths {
        asset_server.reload(p);
    }
    *floor_defs = new_defs;
    atlases.layouts.clear();
    atlases.images.clear();
    dirty.0 = true;
    palette_dirty.0 = true;
}

fn paint_input(
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
    mouse: Res<ButtonInput<MouseButton>>,
    active: Res<ActiveFloor>,
    mut map: ResMut<ViewerFloorMap>,
    mut dirty: ResMut<ViewerDirty>,
) {
    let lmb = mouse.pressed(MouseButton::Left);
    let rmb = mouse.pressed(MouseButton::Right);
    if !lmb && !rmb {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, transform)) = cameras.single() else {
        return;
    };
    let Ok(world) = camera.viewport_to_world_2d(transform, cursor) else {
        return;
    };
    let tile_x = (world.x / TILE_SIZE + GRID_W as f32 / 2.0).floor() as i32;
    let tile_y = (world.y / TILE_SIZE + GRID_H as f32 / 2.0).floor() as i32;
    let new_value = if rmb {
        None
    } else {
        let Some(id) = active.0.clone() else {
            return;
        };
        Some(id)
    };
    let current = map.0.get(tile_x, tile_y).cloned();
    if current == new_value {
        return;
    }
    if map.0.set(tile_x, tile_y, new_value) {
        dirty.0 = true;
    }
}
