#![allow(clippy::type_complexity)]
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::resources::{
    EditingField, EditorCamera, EditorContext, EditorPortalBuffer, EditorPropertyEditBuffer,
    EditorState, EditorTool, ModalConfirmed, ModalKind, ModalState, ModalTextField, UndoOp,
    UndoStack,
};
use crate::editor::serializer::serialize_and_save;
use crate::player::components::Player;
use crate::world::animation::VisualOffset;
use crate::world::components::{
    OverworldObject, SpaceResident, TilePosition, ViewPosition, WorldVisual,
};
use crate::world::map_layout::{PortalDefinition, SpaceDefinitions, TileCoordinate};
use crate::world::object_definitions::{OverworldObjectDefinition, OverworldObjectDefinitions};
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::{RuntimeSpace, SpaceManager};
use crate::world::setup::{
    instantiate_space, spawn_overworld_object, sprite_for_definition, world_visual_for_definition,
};
use crate::world::WorldConfig;

// ── Visuals helper (public so undo.rs can use it) ────────────────────────────

pub fn insert_editor_visuals_pub(
    entity_commands: &mut EntityCommands,
    asset_server: &AssetServer,
    def: &OverworldObjectDefinition,
    world_config: &WorldConfig,
    tile: TilePosition,
    camera: &EditorCamera,
) {
    insert_editor_visuals(
        entity_commands,
        asset_server,
        def,
        world_config,
        tile,
        camera,
    );
}

fn insert_editor_visuals(
    entity_commands: &mut EntityCommands,
    asset_server: &AssetServer,
    def: &OverworldObjectDefinition,
    world_config: &WorldConfig,
    tile: TilePosition,
    camera: &EditorCamera,
) {
    let effective_size = world_config.tile_size * camera.zoom_level;
    let sprite = sprite_for_definition(asset_server, def, world_config);
    let visual = world_visual_for_definition(def, world_config.tile_size);
    let anchor_y_offset = if def.render.y_sort {
        -effective_size * 0.5
    } else {
        0.0
    };
    let x = (tile.x as f32 - camera.center.x) * effective_size;
    let y = (tile.y as f32 - camera.center.y) * effective_size + anchor_y_offset;
    entity_commands.try_insert((
        sprite,
        visual,
        Transform::from_xyz(x, y, def.render.z_index).with_scale(Vec3::splat(camera.zoom_level)),
    ));
    if def.render.y_sort {
        entity_commands.try_insert(bevy::sprite::Anchor::BOTTOM_CENTER);
    }
}

// ── Initialization ────────────────────────────────────────────────────────────

pub fn init_editor_context(
    mut commands: Commands,
    world_config: Res<WorldConfig>,
    space_manager: Res<SpaceManager>,
    space_definitions: Res<SpaceDefinitions>,
    mut editor_camera: ResMut<EditorCamera>,
) {
    let space_id = world_config.current_space_id;
    let authored_id = space_manager
        .get(space_id)
        .map(|s| s.authored_id.clone())
        .unwrap_or_else(|| space_definitions.bootstrap_space_id.clone());

    editor_camera.center = Vec2::new(
        world_config.map_width as f32 * 0.5,
        world_config.map_height as f32 * 0.5,
    );

    commands.insert_resource(EditorContext {
        space_id,
        authored_id,
        map_width: world_config.map_width,
        map_height: world_config.map_height,
        fill_object_type: world_config.fill_object_type.clone(),
    });
}

pub fn init_portal_buffer(
    editor_context: Res<EditorContext>,
    space_definitions: Res<SpaceDefinitions>,
    mut portal_buffer: ResMut<EditorPortalBuffer>,
) {
    portal_buffer.portals = space_definitions
        .get(&editor_context.authored_id)
        .map(|def| def.portals.clone())
        .unwrap_or_default();
}

pub fn attach_editor_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    objects: Query<
        (Entity, &OverworldObject, &TilePosition, &SpaceResident),
        (Without<Transform>, Without<Player>),
    >,
) {
    for (entity, obj, tile, resident) in &objects {
        // Only attach visuals for objects in the active editing space
        if resident.space_id != editor_context.space_id {
            continue;
        }
        let Some(def) = definitions.get(&obj.definition_id) else {
            continue;
        };
        insert_editor_visuals(
            &mut commands.entity(entity),
            &asset_server,
            def,
            &world_config,
            *tile,
            &editor_camera,
        );
    }
}

// ── Camera ────────────────────────────────────────────────────────────────────

pub fn handle_editor_camera_pan(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    modal_state: Res<ModalState>,
    editor_state: Res<EditorState>,
    mut editor_camera: ResMut<EditorCamera>,
    editor_context: Res<EditorContext>,
) {
    if modal_state.active.is_some() || editor_state.palette_filter_focused {
        return;
    }

    let mut delta = Vec2::ZERO;
    if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::ArrowUp) {
        delta.y += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) || keyboard.pressed(KeyCode::ArrowDown) {
        delta.y -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::ArrowLeft) {
        delta.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) || keyboard.pressed(KeyCode::ArrowRight) {
        delta.x += 1.0;
    }

    let pan_speed = editor_camera.pan_speed_tiles_per_sec;
    editor_camera.center += delta * pan_speed * time.delta_secs();
    editor_camera.center = editor_camera.center.clamp(
        Vec2::ZERO,
        Vec2::new(
            (editor_context.map_width - 1) as f32,
            (editor_context.map_height - 1) as f32,
        ),
    );
}

pub fn handle_editor_zoom(
    mut mouse_wheel: bevy::ecs::message::MessageReader<MouseWheel>,
    modal_state: Res<ModalState>,
    mut editor_camera: ResMut<EditorCamera>,
) {
    if modal_state.active.is_some() {
        return;
    }
    for event in mouse_wheel.read() {
        let factor = if event.y > 0.0 {
            1.15_f32
        } else {
            1.0 / 1.15_f32
        };
        editor_camera.zoom_level = (editor_camera.zoom_level * factor).clamp(0.25, 4.0);
    }
}

pub fn sync_tile_transforms_editor(
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut query: Query<
        (
            &SpaceResident,
            &TilePosition,
            &WorldVisual,
            &mut Transform,
            Option<&VisualOffset>,
        ),
        Without<Player>,
    >,
) {
    let effective_size = world_config.tile_size * editor_camera.zoom_level;
    for (space_resident, tile_position, world_visual, mut transform, visual_offset) in &mut query {
        let is_active = space_resident.space_id == editor_context.space_id;
        let z = if !is_active {
            -10_000.0
        } else if world_visual.y_sort {
            crate::world::systems::y_sort_z(tile_position.y, tile_position.z)
        } else {
            crate::world::systems::flat_floor_z(world_visual.z_index, tile_position.z)
        };
        let anchor_y_offset = if world_visual.y_sort {
            -effective_size * 0.5
        } else {
            0.0
        };
        let entity_offset = visual_offset.map_or(Vec2::ZERO, |o| o.current);
        transform.translation = Vec3::new(
            (tile_position.x as f32 - editor_camera.center.x) * effective_size + entity_offset.x,
            (tile_position.y as f32 - editor_camera.center.y) * effective_size
                + anchor_y_offset
                + entity_offset.y,
            z,
        );
        transform.scale = Vec3::splat(editor_camera.zoom_level);
    }
}

fn cursor_to_tile(
    cursor: Vec2,
    window: &Window,
    world_config: &WorldConfig,
    camera: &EditorCamera,
) -> TilePosition {
    let effective_size = world_config.tile_size * camera.zoom_level;
    let center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let offset = cursor - center;
    TilePosition::ground(
        (camera.center.x + offset.x / effective_size).round() as i32,
        (camera.center.y - offset.y / effective_size).round() as i32,
    )
}

// ── Left / right click ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn handle_editor_left_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut editor_state: ResMut<EditorState>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut undo_stack: ResMut<UndoStack>,
    mut modal_state: ResMut<ModalState>,
    existing_objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    interactions: Query<&Interaction>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if interactions.iter().any(|i| *i != Interaction::None) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);
    if tile.x < 0
        || tile.y < 0
        || tile.x >= editor_context.map_width
        || tile.y >= editor_context.map_height
    {
        return;
    }

    if editor_state.current_tool == EditorTool::Portal {
        modal_state.active = Some(ModalKind::PortalCreate);
        modal_state.portal_source_tile = Some(tile);
        modal_state.text_fields = vec![
            ModalTextField {
                label: "Portal ID".into(),
                value: String::new(),
                placeholder: "portal_to_dungeon".into(),
                numeric_only: false,
            },
            ModalTextField {
                label: "Destination Space ID".into(),
                value: String::new(),
                placeholder: "starter_cellar".into(),
                numeric_only: false,
            },
            ModalTextField {
                label: "Destination Tile X".into(),
                value: String::new(),
                placeholder: "7".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Destination Tile Y".into(),
                value: String::new(),
                placeholder: "9".into(),
                numeric_only: true,
            },
        ];
        modal_state.focused_field = 0;
        modal_state.error_message = None;
        modal_state.confirm_triggered = false;
        modal_state.confirmed = None;
        return;
    }

    let existing = existing_objects
        .iter()
        .find(|(_, resident, pos)| resident.space_id == editor_context.space_id && **pos == tile);
    if let Some((obj, _, _)) = existing {
        editor_state.selected_object_id = Some(obj.object_id);
        editor_state.selected_type_id = None;
        if let Some(props) = object_registry.properties(obj.object_id) {
            prop_buffer.object_id = Some(obj.object_id);
            prop_buffer.entries = props.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            prop_buffer.entries.sort_by(|a, b| a.0.cmp(&b.0));
        } else {
            prop_buffer.object_id = Some(obj.object_id);
            prop_buffer.entries.clear();
        }
        prop_buffer.editing_index = None;
        prop_buffer.edit_text.clear();
        return;
    }

    let Some(ref type_id) = editor_state.selected_type_id.clone() else {
        return;
    };
    let Some(def) = definitions.get(type_id) else {
        return;
    };

    let object_id = object_registry.allocate_runtime_id(type_id.clone());
    let entity = spawn_overworld_object(
        &mut commands,
        &definitions,
        object_id,
        type_id,
        None,
        editor_context.space_id,
        tile,
        None,
    );
    insert_editor_visuals(
        &mut commands.entity(entity),
        &asset_server,
        def,
        &world_config,
        tile,
        &editor_camera,
    );
    undo_stack.push_undo(UndoOp::Despawn { object_id });
    editor_state.dirty = true;
}

#[allow(clippy::too_many_arguments)]
pub fn handle_editor_right_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut editor_state: ResMut<EditorState>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut undo_stack: ResMut<UndoStack>,
    mut portal_buffer: ResMut<EditorPortalBuffer>,
    objects: Query<(Entity, &OverworldObject, &SpaceResident, &TilePosition)>,
    object_registry: Res<ObjectRegistry>,
    mut commands: Commands,
    interactions: Query<&Interaction>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    if interactions.iter().any(|i| *i != Interaction::None) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);

    if editor_state.current_tool == EditorTool::Portal {
        if let Some(idx) = portal_buffer
            .portals
            .iter()
            .position(|p| p.source.x == tile.x && p.source.y == tile.y)
        {
            let portal = portal_buffer.portals.remove(idx);
            undo_stack.push_undo(UndoOp::AddPortal { portal });
            editor_state.dirty = true;
        }
        return;
    }

    let hit = objects.iter().find(|(_, _, resident, pos)| {
        resident.space_id == editor_context.space_id && **pos == tile
    });
    if let Some((entity, obj, _, _)) = hit {
        let deleted_id = obj.object_id;
        let type_id = object_registry
            .type_id(deleted_id)
            .unwrap_or(&obj.definition_id)
            .to_owned();
        let properties = object_registry
            .properties(deleted_id)
            .cloned()
            .unwrap_or_default();
        undo_stack.push_undo(UndoOp::Spawn {
            type_id,
            space_id: editor_context.space_id,
            tile,
            properties,
        });
        commands.entity(entity).despawn();
        if editor_state.selected_object_id == Some(deleted_id) {
            editor_state.selected_object_id = None;
            prop_buffer.object_id = None;
            prop_buffer.entries.clear();
            prop_buffer.editing_index = None;
        }
        editor_state.dirty = true;
    }
}

// ── Keyboard ──────────────────────────────────────────────────────────────────

pub fn handle_editor_keyboard_input(
    mut keyboard_events: bevy::ecs::message::MessageReader<KeyboardInput>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut editor_state: ResMut<EditorState>,
) {
    if editor_state.palette_filter_focused {
        for event in keyboard_events.read() {
            if !event.state.is_pressed() {
                continue;
            }
            match event.key_code {
                KeyCode::Escape => {
                    editor_state.palette_filter_focused = false;
                }
                KeyCode::Backspace => {
                    editor_state.palette_filter.pop();
                }
                _ => {
                    if event.repeat {
                        continue;
                    }
                    match &event.logical_key {
                        Key::Character(ch) => {
                            editor_state.palette_filter.push_str(ch.as_str());
                        }
                        Key::Space => {
                            editor_state.palette_filter.push(' ');
                        }
                        _ => {}
                    }
                }
            }
        }
        return;
    }

    let Some(editing_index) = prop_buffer.editing_index else {
        return;
    };
    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match event.key_code {
            KeyCode::Escape => {
                prop_buffer.editing_index = None;
                prop_buffer.edit_text.clear();
            }
            KeyCode::Enter | KeyCode::Tab => {
                commit_edit(
                    &mut prop_buffer,
                    &mut object_registry,
                    &mut editor_state,
                    editing_index,
                );
            }
            KeyCode::Backspace => {
                prop_buffer.edit_text.pop();
            }
            _ => {
                if event.repeat {
                    continue;
                }
                match &event.logical_key {
                    Key::Character(ch) => {
                        prop_buffer.edit_text.push_str(ch.as_str());
                    }
                    Key::Space => {
                        prop_buffer.edit_text.push(' ');
                    }
                    _ => {}
                }
            }
        }
    }
}

fn commit_edit(
    prop_buffer: &mut EditorPropertyEditBuffer,
    object_registry: &mut ObjectRegistry,
    editor_state: &mut EditorState,
    editing_index: usize,
) {
    let text = prop_buffer.edit_text.clone();
    prop_buffer.editing_index = None;
    prop_buffer.edit_text.clear();
    if let Some(entry) = prop_buffer.entries.get_mut(editing_index) {
        match prop_buffer.editing_field {
            EditingField::Value => entry.1 = text,
            EditingField::Key => entry.0 = text,
        }
    }
    if let Some(object_id) = prop_buffer.object_id {
        let props = prop_buffer
            .entries
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        object_registry.set_properties(object_id, props);
        editor_state.dirty = true;
    }
}

pub fn handle_editor_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    modal_state: Res<ModalState>,
    mut editor_state: ResMut<EditorState>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }
    if modal_state.active.is_some() {
        return;
    }
    if prop_buffer.editing_index.is_some() {
        return;
    }
    if editor_state.palette_filter_focused {
        return;
    }

    if editor_state.current_tool != EditorTool::Brush {
        editor_state.current_tool = EditorTool::Brush;
    } else if editor_state.selected_type_id.is_some() {
        editor_state.selected_type_id = None;
    } else if editor_state.selected_object_id.is_some() {
        editor_state.selected_object_id = None;
        prop_buffer.object_id = None;
        prop_buffer.entries.clear();
    }
}

pub fn handle_editor_save(
    keyboard: Res<ButtonInput<KeyCode>>,
    modal_state: Res<ModalState>,
    mut editor_state: ResMut<EditorState>,
    editor_context: Res<EditorContext>,
    portal_buffer: Res<EditorPortalBuffer>,
    object_registry: Res<ObjectRegistry>,
    objects: Query<(&OverworldObject, &SpaceResident, &TilePosition)>,
) {
    if modal_state.active.is_some() {
        return;
    }
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if ctrl && !shift && keyboard.just_pressed(KeyCode::KeyS) {
        serialize_and_save(&editor_context, &portal_buffer, &object_registry, &objects);
        editor_state.dirty = false;
        info!("Saved map '{}'", editor_context.authored_id);
    }
}

// ── Dialog openers (keyboard shortcuts) ──────────────────────────────────────

pub fn open_file_dialog_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_context: Res<EditorContext>,
    mut modal_state: ResMut<ModalState>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl || !keyboard.just_pressed(KeyCode::KeyO) || modal_state.active.is_some() {
        return;
    }
    open_file_dialog_impl(&editor_context, &mut modal_state);
}

pub fn open_save_as_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_context: Res<EditorContext>,
    mut modal_state: ResMut<ModalState>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if !(ctrl && shift && keyboard.just_pressed(KeyCode::KeyS)) || modal_state.active.is_some() {
        return;
    }
    open_save_as_impl(&editor_context, &mut modal_state);
}

pub fn open_file_dialog_impl(editor_context: &EditorContext, modal_state: &mut ModalState) {
    let mut items: Vec<String> = std::fs::read_dir("assets/maps")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("yaml") {
                        p.file_stem()?.to_str().map(|s| s.to_owned())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort();
    let selected = items.iter().position(|s| s == &editor_context.authored_id);
    *modal_state = ModalState {
        active: Some(ModalKind::FileOpen),
        list_items: items,
        selected_list_item: selected,
        ..default()
    };
}

pub fn open_save_as_impl(editor_context: &EditorContext, modal_state: &mut ModalState) {
    *modal_state = ModalState {
        active: Some(ModalKind::SaveAs),
        text_fields: vec![ModalTextField {
            label: "Map ID".into(),
            value: editor_context.authored_id.clone(),
            placeholder: "my_map".into(),
            numeric_only: false,
        }],
        ..default()
    };
}

pub fn open_new_map_dialog_impl(modal_state: &mut ModalState) {
    *modal_state = ModalState {
        active: Some(ModalKind::NewMap),
        text_fields: vec![
            ModalTextField {
                label: "Map ID".into(),
                value: String::new(),
                placeholder: "my_dungeon".into(),
                numeric_only: false,
            },
            ModalTextField {
                label: "Width".into(),
                value: String::new(),
                placeholder: "32".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Height".into(),
                value: String::new(),
                placeholder: "24".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Fill Tile Type".into(),
                value: String::new(),
                placeholder: "grass".into(),
                numeric_only: false,
            },
        ],
        ..default()
    };
}

// ── Modal confirm processing ──────────────────────────────────────────────────

pub fn process_modal_confirm(
    mut modal_state: ResMut<ModalState>,
    editor_state: Res<EditorState>,
    definitions: Res<OverworldObjectDefinitions>,
) {
    if !modal_state.confirm_triggered {
        return;
    }
    modal_state.confirm_triggered = false;

    let Some(kind) = modal_state.active else {
        return;
    };
    match kind {
        ModalKind::FileOpen => {
            let Some(idx) = modal_state.selected_list_item else {
                modal_state.error_message = Some("Select a map first.".into());
                return;
            };
            let Some(authored_id) = modal_state.list_items.get(idx).cloned() else {
                return;
            };
            if editor_state.dirty && modal_state.error_message.is_none() {
                modal_state.error_message =
                    Some("Unsaved changes — click Open again to discard.".into());
                return;
            }
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::FileOpen { authored_id });
        }
        ModalKind::SaveAs => {
            let authored_id = modal_state
                .text_fields
                .first()
                .map(|f| f.value.trim().to_owned())
                .unwrap_or_default();
            if authored_id.is_empty() {
                modal_state.error_message = Some("Map ID cannot be empty.".into());
                return;
            }
            if !authored_id.chars().all(|c| c.is_alphanumeric() || c == '_') {
                modal_state.error_message =
                    Some("Map ID: letters, digits, underscores only.".into());
                return;
            }
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::SaveAs { authored_id });
        }
        ModalKind::NewMap => {
            let vals: Vec<String> = modal_state
                .text_fields
                .iter()
                .map(|f| f.value.trim().to_owned())
                .collect();
            let authored_id = vals.first().cloned().unwrap_or_default();
            if authored_id.is_empty()
                || !authored_id.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                modal_state.error_message = Some("Map ID must be non-empty alphanumeric.".into());
                return;
            }
            let width: i32 = match vals.get(1).and_then(|s| s.parse().ok()) {
                Some(v) if v > 0 && v <= 256 => v,
                _ => {
                    modal_state.error_message = Some("Width must be 1–256.".into());
                    return;
                }
            };
            let height: i32 = match vals.get(2).and_then(|s| s.parse().ok()) {
                Some(v) if v > 0 && v <= 256 => v,
                _ => {
                    modal_state.error_message = Some("Height must be 1–256.".into());
                    return;
                }
            };
            let fill_type = vals.get(3).cloned().unwrap_or_else(|| "grass".into());
            if definitions.get(&fill_type).is_none() {
                modal_state.error_message = Some(format!("Unknown fill tile '{fill_type}'."));
                return;
            }
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::NewMap {
                authored_id,
                width,
                height,
                fill_type,
            });
        }
        ModalKind::PortalCreate => {
            let vals: Vec<String> = modal_state
                .text_fields
                .iter()
                .map(|f| f.value.trim().to_owned())
                .collect();
            let id = vals.first().cloned().unwrap_or_default();
            let dest_space_id = vals.get(1).cloned().unwrap_or_default();
            let dest_tile_x: i32 = vals.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let dest_tile_y: i32 = vals.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            if id.is_empty() {
                modal_state.error_message = Some("Portal ID required.".into());
                return;
            }
            if dest_space_id.is_empty() {
                modal_state.error_message = Some("Destination Space ID required.".into());
                return;
            }
            let Some(source_tile) = modal_state.portal_source_tile else {
                return;
            };
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::PortalCreate {
                source_tile,
                id,
                dest_space_id,
                dest_tile_x,
                dest_tile_y,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn apply_modal_confirmed(
    mut modal_state: ResMut<ModalState>,
    mut editor_context: ResMut<EditorContext>,
    mut editor_state: ResMut<EditorState>,
    mut editor_camera: ResMut<EditorCamera>,
    mut world_config: ResMut<WorldConfig>,
    mut space_manager: ResMut<SpaceManager>,
    mut space_definitions: ResMut<SpaceDefinitions>,
    mut portal_buffer: ResMut<EditorPortalBuffer>,
    mut undo_stack: ResMut<UndoStack>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    object_definitions: Res<OverworldObjectDefinitions>,
    object_registry: Res<ObjectRegistry>,
    objects_save: Query<(&OverworldObject, &SpaceResident, &TilePosition)>,
    mut commands: Commands,
) {
    let Some(confirmed) = modal_state.confirmed.take() else {
        return;
    };

    match confirmed {
        ModalConfirmed::FileOpen { authored_id } => {
            if space_definitions.get(&authored_id).is_none()
                && !space_definitions.load_single_from_disk(&authored_id)
            {
                warn!("Could not load map '{authored_id}' from disk");
                return;
            }
            let Some(def) = space_definitions.get(&authored_id).cloned() else {
                return;
            };

            let space_id = if let Some(id) = space_manager.persistent_space_id(&authored_id) {
                id
            } else {
                instantiate_space(
                    &mut commands,
                    &mut space_manager,
                    &def,
                    &object_definitions,
                    None,
                    def.permanence,
                )
            };

            editor_context.space_id = space_id;
            editor_context.authored_id = authored_id.clone();
            editor_context.map_width = def.width;
            editor_context.map_height = def.height;
            editor_context.fill_object_type = def.fill_object_type.clone();
            world_config.current_space_id = space_id;
            world_config.map_width = def.width;
            world_config.map_height = def.height;
            world_config.fill_object_type = def.fill_object_type.clone();
            editor_camera.center = Vec2::new(def.width as f32 * 0.5, def.height as f32 * 0.5);
            portal_buffer.portals = def.portals.clone();
            editor_state.dirty = false;
            editor_state.selected_type_id = None;
            editor_state.selected_object_id = None;
            editor_state.current_tool = EditorTool::Brush;
            prop_buffer.object_id = None;
            prop_buffer.entries.clear();
            prop_buffer.editing_index = None;
            undo_stack.clear();
        }
        ModalConfirmed::SaveAs { authored_id } => {
            editor_context.authored_id = authored_id.clone();
            serialize_and_save(
                &editor_context,
                &portal_buffer,
                &object_registry,
                &objects_save,
            );
            space_definitions.load_single_from_disk(&authored_id);
            editor_state.dirty = false;
            info!("Saved map as '{authored_id}'");
        }
        ModalConfirmed::NewMap {
            authored_id,
            width,
            height,
            fill_type,
        } => {
            let new_space_id = space_manager.allocate_space_id();
            space_manager.insert_space(RuntimeSpace {
                id: new_space_id,
                authored_id: authored_id.clone(),
                width,
                height,
                fill_object_type: fill_type.clone(),
                permanence: crate::world::map_layout::SpacePermanence::Persistent,
                instance_owner: None,
            });
            space_definitions.insert_or_replace(
                crate::world::map_layout::SpaceDefinition::new_empty(
                    authored_id.clone(),
                    width,
                    height,
                    fill_type.clone(),
                ),
            );
            editor_context.space_id = new_space_id;
            editor_context.authored_id = authored_id.clone();
            editor_context.map_width = width;
            editor_context.map_height = height;
            editor_context.fill_object_type = fill_type.clone();
            world_config.current_space_id = new_space_id;
            world_config.map_width = width;
            world_config.map_height = height;
            world_config.fill_object_type = fill_type.clone();
            editor_camera.center = Vec2::new(width as f32 * 0.5, height as f32 * 0.5);
            portal_buffer.portals = vec![];
            editor_state.dirty = true;
            editor_state.selected_type_id = None;
            editor_state.selected_object_id = None;
            editor_state.current_tool = EditorTool::Brush;
            prop_buffer.object_id = None;
            prop_buffer.entries.clear();
            prop_buffer.editing_index = None;
            undo_stack.clear();
        }
        ModalConfirmed::PortalCreate {
            source_tile,
            id,
            dest_space_id,
            dest_tile_x,
            dest_tile_y,
        } => {
            let portal = PortalDefinition {
                id,
                source: TileCoordinate {
                    x: source_tile.x,
                    y: source_tile.y,
                    z: source_tile.z,
                },
                destination_space_id: dest_space_id,
                destination_tile: TileCoordinate {
                    x: dest_tile_x,
                    y: dest_tile_y,
                    z: 0,
                },
                destination_permanence: None,
            };
            let index = portal_buffer.portals.len();
            portal_buffer.portals.push(portal);
            undo_stack.push_undo(UndoOp::RemovePortal { index });
            editor_state.dirty = true;
        }
    }
}

// ── Portal overlays ───────────────────────────────────────────────────────────

pub fn sync_portal_overlays(
    portal_buffer: Res<EditorPortalBuffer>,
    editor_context: Res<EditorContext>,
    markers: Query<Entity, With<crate::editor::resources::EditorPortalMarker>>,
    mut commands: Commands,
) {
    if !portal_buffer.is_changed() && !editor_context.is_changed() {
        return;
    }
    for entity in &markers {
        commands.entity(entity).despawn();
    }
    for (i, portal) in portal_buffer.portals.iter().enumerate() {
        let tile = portal.source.to_tile_position();
        commands.spawn((
            crate::editor::resources::EditorPortalMarker { portal_index: i },
            SpaceResident {
                space_id: editor_context.space_id,
            },
            tile,
            ViewPosition {
                space_id: editor_context.space_id,
                tile,
            },
            WorldVisual {
                z_index: 8.0,
                y_sort: false,
                sprite_height: 0.0,
                rotation_by_facing: false,
            },
            Sprite {
                color: Color::srgba(0.2, 0.6, 1.0, 0.55),
                custom_size: Some(Vec2::splat(48.0)),
                ..default()
            },
            Transform::default(),
        ));
    }
}
