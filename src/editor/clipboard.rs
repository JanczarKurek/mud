//! Editor copy / cut / paste.
//!
//! - **Copy** (Ctrl+C, Ctrl+Shift+C for objects-only) snapshots the current
//!   selection into `EditorClipboard`. Coordinates become relative to the
//!   selection's top-left.
//! - **Cut** (Ctrl+X, Shift = objects-only) does Copy then deletes the
//!   contents, pushed as a single `UndoOp::Composite` so Ctrl+Z restores
//!   everything atomically.
//! - **Paste** (Ctrl+V) flips `EditorState::paste_state.active = true`. The
//!   actual stamp commit lives in `handle_editor_left_click` (so the click
//!   path has single-owner access to commands/registry); the cancel path
//!   lives in `handle_editor_right_click` and `handle_editor_escape`.
//!
//! Authored `MapBehavior` is dropped on copy because behaviors are tied to
//! authored object IDs that don't survive runtime allocation. Multi-tile
//! sprites are captured by their tile origin only.

use bevy::prelude::*;

use crate::editor::resources::{
    EditorClipboard, EditorContext, EditorSelection, EditorState, FragmentFloor, FragmentObject,
    MapFragment, UndoOp, UndoStack,
};
use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::player::components::Player;
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::floor_map::FloorMaps;
use crate::world::object_registry::ObjectRegistry;

/// Build a `MapFragment` from the contents of `selection`. `include_floors`
/// false means produce an objects-only fragment (the Shift modifier).
pub fn fragment_from_selection(
    selection: EditorSelection,
    include_floors: bool,
    objects: impl IntoIterator<Item = (u64, TilePosition)>,
    object_registry: &ObjectRegistry,
    floor_maps: &FloorMaps,
) -> MapFragment {
    let mut fragment = MapFragment {
        width: selection.width(),
        height: selection.height(),
        objects: Vec::new(),
        floors: Vec::new(),
    };
    for (object_id, tile) in objects {
        if !selection.contains(tile.x, tile.y) {
            continue;
        }
        let Some(type_id) = object_registry.type_id(object_id) else {
            continue;
        };
        let properties = object_registry
            .properties(object_id)
            .cloned()
            .unwrap_or_default();
        fragment.objects.push(FragmentObject {
            dx: tile.x - selection.min.x,
            dy: tile.y - selection.min.y,
            z: tile.z,
            type_id: type_id.to_owned(),
            properties,
        });
    }
    if include_floors {
        if let Some(map) = floor_maps.get(selection.space_id, TilePosition::GROUND_FLOOR) {
            for y in selection.min.y..=selection.max.y {
                for x in selection.min.x..=selection.max.x {
                    let cell = map.get(x, y).cloned();
                    fragment.floors.push(FragmentFloor {
                        dx: x - selection.min.x,
                        dy: y - selection.min.y,
                        floor_id: cell,
                    });
                }
            }
        }
    }
    fragment
}

/// Ctrl+C / Ctrl+X / Ctrl+V keyboard handler. Skips when a modal is open
/// (caller already gates on `no_modal`) or when the palette filter has
/// keyboard focus.
#[allow(clippy::too_many_arguments)]
pub fn handle_clipboard_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_context: Res<EditorContext>,
    object_registry: Res<ObjectRegistry>,
    floor_maps: Res<FloorMaps>,
    mut editor_state: ResMut<EditorState>,
    mut clipboard: ResMut<EditorClipboard>,
    mut undo_stack: ResMut<UndoStack>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut commands: Commands,
    objects: Query<(Entity, &OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
) {
    if editor_state.palette_filter_focused {
        return;
    }
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let copy = keyboard.just_pressed(KeyCode::KeyC);
    let cut = keyboard.just_pressed(KeyCode::KeyX);
    let paste = keyboard.just_pressed(KeyCode::KeyV);

    if !(copy || cut || paste) {
        return;
    }

    if paste {
        if clipboard.fragment.is_some() {
            editor_state.paste_state.active = true;
        } else {
            info!("Paste: clipboard is empty");
        }
        return;
    }

    let Some(selection) = editor_state.selection else {
        info!("Copy/Cut: no selection");
        return;
    };
    if selection.space_id != editor_context.space_id {
        info!("Copy/Cut: selection is on a different space");
        return;
    }

    // Collect (object_id, tile) for objects in the active editing space; passed
    // to `fragment_from_selection` which filters by selection bbox.
    let mut object_entries: Vec<(u64, TilePosition, Entity)> = Vec::new();
    for (entity, obj, resident, tile) in &objects {
        if resident.space_id != selection.space_id {
            continue;
        }
        if !selection.contains(tile.x, tile.y) {
            continue;
        }
        object_entries.push((obj.object_id, *tile, entity));
    }

    let include_floors = !shift;
    let fragment = fragment_from_selection(
        selection,
        include_floors,
        object_entries.iter().map(|(id, tile, _)| (*id, *tile)),
        &object_registry,
        &floor_maps,
    );
    if fragment.objects.is_empty() && fragment.floors.is_empty() {
        info!("Copy/Cut: selection is empty");
        return;
    }
    clipboard.fragment = Some(fragment);

    if copy {
        return;
    }

    // Cut path: despawn objects + clear floors, build a Composite undo so a
    // single Ctrl+Z restores everything atomically.
    let mut composite_ops: Vec<UndoOp> = Vec::new();
    for (object_id, tile, entity) in &object_entries {
        let type_id = object_registry
            .type_id(*object_id)
            .map(str::to_owned)
            .unwrap_or_default();
        let properties = object_registry
            .properties(*object_id)
            .cloned()
            .unwrap_or_default();
        composite_ops.push(UndoOp::Spawn {
            type_id,
            space_id: selection.space_id,
            tile: *tile,
            properties,
        });
        commands.entity(*entity).despawn();
    }
    if include_floors {
        if let Some(map) = floor_maps.get(selection.space_id, TilePosition::GROUND_FLOOR) {
            for y in selection.min.y..=selection.max.y {
                for x in selection.min.x..=selection.max.x {
                    let prev = map.get(x, y).cloned();
                    if prev.is_none() {
                        continue;
                    }
                    composite_ops.push(UndoOp::SetFloor {
                        space_id: selection.space_id,
                        z: TilePosition::GROUND_FLOOR,
                        x,
                        y,
                        value: prev,
                    });
                    pending_commands.push(GameCommand::EditorSetFloorTile {
                        space_id: selection.space_id,
                        z: TilePosition::GROUND_FLOOR,
                        x,
                        y,
                        floor_type: None,
                    });
                }
            }
        }
    }
    if !composite_ops.is_empty() {
        undo_stack.push_undo(UndoOp::Composite { ops: composite_ops });
        editor_state.dirty = true;
    }
}

/// Build a fragment from the current selection for the "Save Selection as
/// Template" toolbar button. Returns `None` if the selection is empty or
/// produces an empty fragment. Always includes floors.
pub fn fragment_from_state(
    editor_state: &EditorState,
    editor_context: &EditorContext,
    object_registry: &ObjectRegistry,
    floor_maps: &FloorMaps,
    objects: &Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
) -> Option<MapFragment> {
    let selection = editor_state.selection?;
    if selection.space_id != editor_context.space_id {
        return None;
    }
    let mut object_entries: Vec<(u64, TilePosition)> = Vec::new();
    for (obj, resident, tile) in objects.iter() {
        if resident.space_id != selection.space_id {
            continue;
        }
        if !selection.contains(tile.x, tile.y) {
            continue;
        }
        object_entries.push((obj.object_id, *tile));
    }
    let fragment =
        fragment_from_selection(selection, true, object_entries, object_registry, floor_maps);
    if fragment.objects.is_empty() && fragment.floors.iter().all(|f| f.floor_id.is_none()) {
        None
    } else {
        Some(fragment)
    }
}

/// Stamp the clipboard fragment at `cursor_tile` (the cursor's current tile,
/// which becomes the top-left of the placed fragment). Returns the
/// `Composite` undo op to push (caller decides whether to add to the stack
/// based on whether anything was placed). Out-of-bounds cells are silently
/// clipped.
#[allow(clippy::too_many_arguments)]
pub fn stamp_fragment(
    fragment: &MapFragment,
    cursor_tile: TilePosition,
    editor_context: &EditorContext,
    object_registry: &mut ObjectRegistry,
    object_definitions: &crate::world::object_definitions::OverworldObjectDefinitions,
    world_config: &crate::world::WorldConfig,
    editor_camera: &crate::editor::resources::EditorCamera,
    asset_server: &AssetServer,
    floor_maps: &FloorMaps,
    pending_commands: &mut PendingGameCommands,
    commands: &mut Commands,
) -> Option<UndoOp> {
    let mut undo_ops: Vec<UndoOp> = Vec::new();
    let mut clipped_objects = 0u32;

    for fo in &fragment.objects {
        let tile = TilePosition::new(cursor_tile.x + fo.dx, cursor_tile.y + fo.dy, fo.z);
        if tile.x < 0
            || tile.y < 0
            || tile.x >= editor_context.map_width
            || tile.y >= editor_context.map_height
        {
            clipped_objects += 1;
            continue;
        }
        let Some(def) = object_definitions.get(&fo.type_id) else {
            warn!("Paste: unknown object type '{}'; skipping", fo.type_id);
            continue;
        };
        let new_id = object_registry
            .allocate_runtime_id_with_properties(fo.type_id.clone(), fo.properties.clone());
        let entity = crate::world::setup::spawn_overworld_object(
            commands,
            object_definitions,
            new_id,
            &fo.type_id,
            None,
            editor_context.space_id,
            tile,
            None,
        );
        crate::editor::systems::insert_editor_visuals_pub(
            &mut commands.entity(entity),
            asset_server,
            def,
            world_config,
            tile,
            editor_camera,
        );
        undo_ops.push(UndoOp::Despawn { object_id: new_id });
    }

    for ff in &fragment.floors {
        let x = cursor_tile.x + ff.dx;
        let y = cursor_tile.y + ff.dy;
        if x < 0 || y < 0 || x >= editor_context.map_width || y >= editor_context.map_height {
            continue;
        }
        let prev = floor_maps
            .get(editor_context.space_id, TilePosition::GROUND_FLOOR)
            .and_then(|m| m.get(x, y).cloned());
        if prev == ff.floor_id {
            continue;
        }
        undo_ops.push(UndoOp::SetFloor {
            space_id: editor_context.space_id,
            z: TilePosition::GROUND_FLOOR,
            x,
            y,
            value: prev,
        });
        pending_commands.push(GameCommand::EditorSetFloorTile {
            space_id: editor_context.space_id,
            z: TilePosition::GROUND_FLOOR,
            x,
            y,
            floor_type: ff.floor_id.clone(),
        });
    }

    if clipped_objects > 0 {
        warn!("Paste: {clipped_objects} objects clipped outside map bounds");
    }
    if undo_ops.is_empty() {
        None
    } else {
        Some(UndoOp::Composite { ops: undo_ops })
    }
}

/// Reset paste-mode (called by Esc / right-click handlers and on map exit).
pub fn cancel_paste(editor_state: &mut EditorState) {
    editor_state.paste_state.active = false;
}

/// Translucent preview of the clipboard fragment under the cursor while
/// paste mode is active. Owns its own marker (`EditorPasteGhostMarker`) and
/// despawns its previous-frame entities at the top of every run, so the brush
/// cursor cleanup in `update_editor_cursor_ghost` cannot race-despawn this
/// system's just-spawned ghosts.
#[allow(clippy::too_many_arguments)]
pub fn render_paste_ghost(
    mut commands: Commands,
    mut gizmos: Gizmos,
    asset_server: Res<AssetServer>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    world_config: Res<crate::world::WorldConfig>,
    editor_camera: Res<crate::editor::resources::EditorCamera>,
    editor_context: Res<EditorContext>,
    editor_state: Res<EditorState>,
    clipboard: Res<EditorClipboard>,
    object_definitions: Res<crate::world::object_definitions::OverworldObjectDefinitions>,
    floor_defs: Res<crate::world::floor_definitions::FloorTilesetDefinitions>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
    existing: Query<Entity, With<crate::editor::resources::EditorPasteGhostMarker>>,
) {
    // Always clear last frame's ghost first so paste-mode toggles, fragment
    // swaps, and cursor-off-window states never leave stale visuals behind.
    for entity in &existing {
        commands.entity(entity).despawn();
    }

    if !editor_state.paste_state.active {
        return;
    }
    let Some(fragment) = clipboard.fragment.as_ref() else {
        return;
    };
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor) {
        return;
    }
    let tile = cursor_tile(cursor, window, &world_config, &editor_camera);
    if tile.x < 0
        || tile.y < 0
        || tile.x >= editor_context.map_width
        || tile.y >= editor_context.map_height
    {
        return;
    }
    let effective = world_config.tile_size * editor_camera.zoom_level;
    if effective <= f32::EPSILON {
        return;
    }

    let footprint_min = Vec2::new(
        (tile.x as f32 - 0.5 - editor_camera.center.x) * effective,
        (tile.y as f32 - 0.5 - editor_camera.center.y) * effective,
    );
    let footprint_max = Vec2::new(
        (tile.x as f32 + fragment.width as f32 - 0.5 - editor_camera.center.x) * effective,
        (tile.y as f32 + fragment.height as f32 - 0.5 - editor_camera.center.y) * effective,
    );
    let center = Vec2::new(
        (footprint_min.x + footprint_max.x) * 0.5,
        (footprint_min.y + footprint_max.y) * 0.5,
    );
    let size = Vec2::new(
        footprint_max.x - footprint_min.x,
        footprint_max.y - footprint_min.y,
    );

    // Faint blue tint over the whole footprint — instantly readable even when
    // the fragment is sparse (objects-only, single tile, etc.).
    commands.spawn((
        crate::editor::resources::EditorPasteGhostMarker,
        Sprite::from_color(Color::srgba(0.30, 0.65, 1.0, 0.18), size),
        Transform::from_xyz(center.x, center.y, 90.0),
    ));

    // Bright outline. Bevy's gizmo rect_2d is 1px wide; draw four nested
    // rects at small offsets to fake a thicker stroke.
    let outline = Color::srgba(0.95, 0.85, 0.30, 1.0);
    for offset in 0..3 {
        let pad = offset as f32;
        gizmos.rect_2d(
            Isometry2d::from_translation(center),
            size + Vec2::splat(pad * 2.0),
            outline,
        );
    }

    for ff in &fragment.floors {
        let Some(id) = ff.floor_id.as_ref() else {
            continue;
        };
        let Some(def) = floor_defs.get(id) else {
            continue;
        };
        let cx = (tile.x as f32 + ff.dx as f32 - editor_camera.center.x) * effective;
        let cy = (tile.y as f32 + ff.dy as f32 - editor_camera.center.y) * effective;
        let fill = def.debug_color().with_alpha(0.45);
        commands.spawn((
            crate::editor::resources::EditorPasteGhostMarker,
            Sprite::from_color(fill, Vec2::splat(effective * 0.92)),
            Transform::from_xyz(cx, cy, 100.0),
        ));
    }
    for fo in &fragment.objects {
        let Some(def) = object_definitions.get(&fo.type_id) else {
            continue;
        };
        let mut sprite =
            crate::world::setup::sprite_for_definition(&asset_server, def, &world_config);
        sprite.color = sprite.color.with_alpha(0.6);
        let anchor_y_offset = if def.render.y_sort {
            -effective * 0.5
        } else {
            0.0
        };
        let cx = (tile.x as f32 + fo.dx as f32 - editor_camera.center.x) * effective;
        let cy = (tile.y as f32 + fo.dy as f32 - editor_camera.center.y) * effective;
        let z_base = if def.render.y_sort {
            crate::world::systems::y_sort_z(tile.y + fo.dy, fo.z)
        } else {
            crate::world::systems::flat_floor_z(def.render.z_index, fo.z)
        };
        let z = z_base + 50.0;
        let mut entity = commands.spawn((
            crate::editor::resources::EditorPasteGhostMarker,
            sprite,
            Transform::from_xyz(cx, cy + anchor_y_offset, z)
                .with_scale(Vec3::splat(editor_camera.zoom_level)),
        ));
        if def.render.y_sort {
            entity.insert(bevy::sprite::Anchor::BOTTOM_CENTER);
        }
    }
}

/// Dedicated paste-mode commit system. Runs before `handle_editor_left_click`
/// so the click is consumed by paste and not by the brush placement path.
/// Lives separately from `handle_editor_left_click` because that fn already
/// sits at Bevy's system-param-arity ceiling.
#[allow(clippy::too_many_arguments)]
pub fn handle_editor_paste_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    world_config: Res<crate::world::WorldConfig>,
    editor_camera: Res<crate::editor::resources::EditorCamera>,
    editor_context: Res<EditorContext>,
    object_definitions: Res<crate::world::object_definitions::OverworldObjectDefinitions>,
    floor_maps: Res<FloorMaps>,
    clipboard: Res<EditorClipboard>,
    mut editor_state: ResMut<EditorState>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut undo_stack: ResMut<UndoStack>,
    mut pending_commands: ResMut<PendingGameCommands>,
    asset_server: Res<AssetServer>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
    mut commands: Commands,
) {
    if !editor_state.paste_state.active {
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor) {
        return;
    }
    let tile = cursor_tile(cursor, window, &world_config, &editor_camera);
    if tile.x < 0
        || tile.y < 0
        || tile.x >= editor_context.map_width
        || tile.y >= editor_context.map_height
    {
        return;
    }
    let Some(fragment) = clipboard.fragment.as_ref() else {
        return;
    };
    if let Some(undo) = stamp_fragment(
        fragment,
        tile,
        &editor_context,
        &mut object_registry,
        &object_definitions,
        &world_config,
        &editor_camera,
        &asset_server,
        &floor_maps,
        &mut pending_commands,
        &mut commands,
    ) {
        undo_stack.push_undo(undo);
        editor_state.dirty = true;
    }
}

fn cursor_tile(
    cursor: Vec2,
    window: &Window,
    world_config: &crate::world::WorldConfig,
    camera: &crate::editor::resources::EditorCamera,
) -> TilePosition {
    let effective = world_config.tile_size * camera.zoom_level;
    let center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let offset = cursor - center;
    TilePosition::ground(
        (camera.center.x + offset.x / effective).round() as i32,
        (camera.center.y - offset.y / effective).round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::components::SpaceId;

    fn make_fragment() -> MapFragment {
        let mut props = std::collections::HashMap::new();
        props.insert("color".to_owned(), "blue".to_owned());
        MapFragment {
            width: 3,
            height: 2,
            objects: vec![
                FragmentObject {
                    dx: 0,
                    dy: 0,
                    z: 0,
                    type_id: "tree".to_owned(),
                    properties: std::collections::HashMap::new(),
                },
                FragmentObject {
                    dx: 2,
                    dy: 1,
                    z: 1,
                    type_id: "lamp".to_owned(),
                    properties: props,
                },
            ],
            floors: vec![
                FragmentFloor {
                    dx: 0,
                    dy: 0,
                    floor_id: Some("grass".to_owned()),
                },
                FragmentFloor {
                    dx: 1,
                    dy: 0,
                    floor_id: None,
                },
            ],
        }
    }

    #[test]
    fn fragment_yaml_round_trip() {
        let original = make_fragment();
        let yaml = serde_yaml::to_string(&original).expect("serialize");
        let parsed: MapFragment = serde_yaml::from_str(&yaml).expect("deserialize");
        assert_eq!(original.width, parsed.width);
        assert_eq!(original.height, parsed.height);
        assert_eq!(original.objects.len(), parsed.objects.len());
        assert_eq!(original.floors.len(), parsed.floors.len());
        for (a, b) in original.objects.iter().zip(&parsed.objects) {
            assert_eq!(a.dx, b.dx);
            assert_eq!(a.dy, b.dy);
            assert_eq!(a.z, b.z);
            assert_eq!(a.type_id, b.type_id);
            assert_eq!(a.properties, b.properties);
        }
        for (a, b) in original.floors.iter().zip(&parsed.floors) {
            assert_eq!(a.dx, b.dx);
            assert_eq!(a.dy, b.dy);
            assert_eq!(a.floor_id, b.floor_id);
        }
    }

    #[test]
    fn fragment_from_selection_relative_coords() {
        use crate::world::floor_map::FloorMap;
        let space_id = SpaceId(7);
        let selection = EditorSelection {
            space_id,
            min: TilePosition::ground(10, 20),
            max: TilePosition::ground(12, 21),
        };
        let mut registry = ObjectRegistry::default();
        let id_a = registry.allocate_runtime_id("rock");
        let id_b = registry.allocate_runtime_id("tree");
        let mut floor_maps = FloorMaps::default();
        let mut map = FloorMap::new_filled(64, 64, None);
        map.set(10, 20, Some("grass".to_owned()));
        map.set(11, 20, Some("grass".to_owned()));
        floor_maps.insert(space_id, TilePosition::GROUND_FLOOR, map);

        let frag = fragment_from_selection(
            selection,
            true,
            vec![
                (id_a, TilePosition::ground(10, 20)),
                (id_b, TilePosition::new(12, 21, 1)),
                // Out of selection — must be filtered.
                (id_a, TilePosition::ground(0, 0)),
            ],
            &registry,
            &floor_maps,
        );
        assert_eq!(frag.width, 3);
        assert_eq!(frag.height, 2);
        // Both objects in-selection are captured with relative coords.
        let dxs: Vec<(i32, i32, i32)> = frag.objects.iter().map(|o| (o.dx, o.dy, o.z)).collect();
        assert!(dxs.contains(&(0, 0, 0)));
        assert!(dxs.contains(&(2, 1, 1)));
        // Floors include the whole bbox (3 * 2 = 6 cells).
        assert_eq!(frag.floors.len(), 6);
        let first = frag
            .floors
            .iter()
            .find(|f| f.dx == 0 && f.dy == 0)
            .expect("origin");
        assert_eq!(first.floor_id.as_deref(), Some("grass"));
    }
}
