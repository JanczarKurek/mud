use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::resources::{EditingField, EditorCamera, EditorContext, EditorPropertyEditBuffer, EditorState};
use crate::editor::serializer::serialize_and_save;
use crate::world::animation::VisualOffset;
use crate::world::components::{OverworldObject, SpaceResident, TilePosition, WorldVisual};
use crate::world::map_layout::SpaceDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::SpaceManager;
use crate::world::setup::{spawn_overworld_object, sprite_for_definition, world_visual_for_definition};
use crate::world::WorldConfig;
use crate::player::components::Player;

/// Initialize EditorContext from the current WorldConfig and SpaceManager.
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

/// Attach `Sprite`, `WorldVisual`, and `Transform` to an entity for editor rendering.
/// The transform is set to the correct screen-space position immediately so no
/// one-frame flash at the origin occurs before `sync_tile_transforms_editor` runs.
fn insert_editor_visuals(
    entity_commands: &mut EntityCommands,
    asset_server: &AssetServer,
    def: &crate::world::object_definitions::OverworldObjectDefinition,
    world_config: &WorldConfig,
    tile: TilePosition,
    camera: &EditorCamera,
) {
    let sprite = sprite_for_definition(asset_server, def, world_config);
    let visual = world_visual_for_definition(def, world_config.tile_size);
    let anchor_y_offset = if def.render.y_sort { -world_config.tile_size * 0.5 } else { 0.0 };
    let x = (tile.x as f32 - camera.center.x) * world_config.tile_size;
    let y = (tile.y as f32 - camera.center.y) * world_config.tile_size + anchor_y_offset;
    entity_commands.insert((
        sprite,
        visual,
        Transform::from_xyz(x, y, def.render.z_index),
    ));
    if def.render.y_sort {
        entity_commands.insert(bevy::sprite::Anchor::BOTTOM_CENTER);
    }
}

/// Add visual components to existing server-side OverworldObject entities that lack them
/// so they can be rendered in the editor.
pub fn attach_editor_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    objects: Query<(Entity, &OverworldObject, &TilePosition), (With<SpaceResident>, Without<Transform>, Without<Player>)>,
) {
    for (entity, obj, tile) in &objects {
        let Some(def) = definitions.get(&obj.definition_id) else {
            continue;
        };
        insert_editor_visuals(&mut commands.entity(entity), &asset_server, def, &world_config, *tile, &editor_camera);
    }
}

/// Pan the editor camera with WASD / arrow keys.
pub fn handle_editor_camera_pan(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut editor_camera: ResMut<EditorCamera>,
    editor_context: Res<EditorContext>,
) {
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
    editor_camera.center +=
        delta * pan_speed * time.delta_secs();
    editor_camera.center = editor_camera.center.clamp(
        Vec2::ZERO,
        Vec2::new(
            (editor_context.map_width - 1) as f32,
            (editor_context.map_height - 1) as f32,
        ),
    );
}

/// Position all world entities relative to the editor camera.
/// Mirrors `sync_tile_transforms` but uses `EditorCamera` as the center reference.
pub fn sync_tile_transforms_editor(
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut query: Query<
        (&SpaceResident, &TilePosition, &WorldVisual, &mut Transform, Option<&VisualOffset>),
        Without<Player>,
    >,
) {
    for (space_resident, tile_position, world_visual, mut transform, visual_offset) in &mut query {
        let is_active = space_resident.space_id == editor_context.space_id;

        let z = if !is_active {
            -10_000.0
        } else if world_visual.y_sort {
            1.0 - tile_position.y as f32 * 0.01
        } else {
            world_visual.z_index
        };

        let anchor_y_offset = if world_visual.y_sort {
            -world_config.tile_size * 0.5
        } else {
            0.0
        };

        let entity_offset = visual_offset.map_or(Vec2::ZERO, |o| o.current);

        transform.translation = Vec3::new(
            (tile_position.x as f32 - editor_camera.center.x) * world_config.tile_size
                + entity_offset.x,
            (tile_position.y as f32 - editor_camera.center.y) * world_config.tile_size
                + anchor_y_offset
                + entity_offset.y,
            z,
        );
    }
}

/// Convert a cursor position to a tile coordinate using the editor camera.
fn cursor_to_tile(
    cursor: Vec2,
    window: &Window,
    world_config: &WorldConfig,
    editor_camera: &EditorCamera,
) -> TilePosition {
    let window_center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let offset = cursor - window_center;
    let tile_x = (editor_camera.center.x + offset.x / world_config.tile_size).round() as i32;
    let tile_y = (editor_camera.center.y - offset.y / world_config.tile_size).round() as i32;
    TilePosition::new(tile_x, tile_y)
}

/// Left-click: place selected object type OR select an existing object.
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
    existing_objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    // Detect whether we clicked on a UI element
    interactions: Query<&Interaction>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    // Bail if any UI element is being hovered/pressed
    if interactions.iter().any(|i| *i != Interaction::None) {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };

    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);

    if tile.x < 0
        || tile.y < 0
        || tile.x >= editor_context.map_width
        || tile.y >= editor_context.map_height
    {
        return;
    }

    // Check if there's already an object at this tile.
    let existing = existing_objects.iter().find(|(_, resident, pos)| {
        resident.space_id == editor_context.space_id && **pos == tile
    });

    if let Some((obj, _, _)) = existing {
        // Select this object and show its properties.
        let obj_id = obj.object_id;
        editor_state.selected_object_id = Some(obj_id);
        editor_state.selected_type_id = None; // clear brush when selecting

        // Populate property buffer.
        if let Some(props) = object_registry.properties(obj_id) {
            prop_buffer.object_id = Some(obj_id);
            prop_buffer.entries = props.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            prop_buffer.entries.sort_by(|a, b| a.0.cmp(&b.0));
        } else {
            prop_buffer.object_id = Some(obj_id);
            prop_buffer.entries.clear();
        }
        prop_buffer.editing_index = None;
        prop_buffer.edit_text.clear();
        return;
    }

    // Nothing there — place selected type if a brush is active.
    let Some(ref type_id) = editor_state.selected_type_id.clone() else {
        return;
    };

    let Some(def) = definitions.get(type_id) else {
        return;
    };

    let object_id =
        object_registry.allocate_runtime_id(type_id.clone());
    let entity = spawn_overworld_object(
        &mut commands,
        &definitions,
        object_id,
        type_id,
        None,
        editor_context.space_id,
        tile,
    );

    insert_editor_visuals(&mut commands.entity(entity), &asset_server, def, &world_config, tile, &editor_camera);

    editor_state.dirty = true;
}

/// Right-click: delete the object under the cursor.
pub fn handle_editor_right_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut editor_state: ResMut<EditorState>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    objects: Query<(Entity, &OverworldObject, &SpaceResident, &TilePosition)>,
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
    let Some(cursor) = window.cursor_position() else { return };

    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);

    let hit = objects.iter().find(|(_, _, resident, pos)| {
        resident.space_id == editor_context.space_id && **pos == tile
    });

    if let Some((entity, obj, _, _)) = hit {
        let deleted_id = obj.object_id;
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

/// Handle keyboard input for property text editing.
pub fn handle_editor_keyboard_input(
    mut keyboard_events: bevy::ecs::message::MessageReader<KeyboardInput>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut editor_state: ResMut<EditorState>,
) {
    let Some(editing_index) = prop_buffer.editing_index else {
        return;
    };

    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }

        match event.key_code {
            KeyCode::Escape => {
                // Cancel edit, restore original value.
                prop_buffer.editing_index = None;
                prop_buffer.edit_text.clear();
            }
            KeyCode::Enter | KeyCode::Tab => {
                // Commit edit.
                commit_edit(&mut prop_buffer, &mut object_registry, &mut editor_state, editing_index);
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

    // Write back to registry immediately.
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

/// Escape key: deselect brush or selected object.
pub fn handle_editor_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut editor_state: ResMut<EditorState>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }

    // If editing a property, escape is handled in keyboard_input; don't double-process.
    if prop_buffer.editing_index.is_some() {
        return;
    }

    if editor_state.selected_type_id.is_some() {
        editor_state.selected_type_id = None;
    } else if editor_state.selected_object_id.is_some() {
        editor_state.selected_object_id = None;
        prop_buffer.object_id = None;
        prop_buffer.entries.clear();
    }
}

/// Ctrl+S: serialize and save the current space to YAML.
pub fn handle_editor_save(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut editor_state: ResMut<EditorState>,
    editor_context: Res<EditorContext>,
    space_definitions: Res<SpaceDefinitions>,
    object_registry: Res<ObjectRegistry>,
    objects: Query<(&OverworldObject, &SpaceResident, &TilePosition)>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::KeyS)) {
        return;
    }

    serialize_and_save(&editor_context, &space_definitions, &object_registry, &objects);
    editor_state.dirty = false;
    info!("Saved map '{}'", editor_context.authored_id);
}
