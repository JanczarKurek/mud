use bevy::prelude::*;

use crate::editor::resources::{
    EditorCamera, EditorPortalBuffer, EditorState, ModalState, UndoOp, UndoStack,
};
use crate::editor::systems::insert_editor_visuals_pub;
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::spawn_overworld_object;
use crate::world::WorldConfig;

#[allow(clippy::too_many_arguments)]
pub fn handle_undo_redo(
    keyboard: Res<ButtonInput<KeyCode>>,
    modal_state: Res<ModalState>,
    mut undo_stack: ResMut<UndoStack>,
    mut editor_state: ResMut<EditorState>,
    mut portal_buffer: ResMut<EditorPortalBuffer>,
    mut commands: Commands,
    mut object_registry: ResMut<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    asset_server: Res<AssetServer>,
    objects: Query<(Entity, &OverworldObject, &SpaceResident, &TilePosition)>,
) {
    if modal_state.active.is_some() {
        return;
    }

    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    let do_undo =
        editor_state.undo_requested || (ctrl && !shift && keyboard.just_pressed(KeyCode::KeyZ));
    let do_redo = editor_state.redo_requested
        || ctrl
            && (keyboard.just_pressed(KeyCode::KeyY)
                || (shift && keyboard.just_pressed(KeyCode::KeyZ)));
    editor_state.undo_requested = false;
    editor_state.redo_requested = false;

    if do_undo {
        if let Some(op) = undo_stack.undo_ops.pop() {
            let inverse = execute_op(
                op,
                &mut portal_buffer,
                &mut commands,
                &mut object_registry,
                &definitions,
                &world_config,
                &editor_camera,
                &asset_server,
                &objects,
            );
            undo_stack.redo_ops.push(inverse);
            editor_state.dirty = true;
        }
    } else if do_redo {
        if let Some(op) = undo_stack.redo_ops.pop() {
            let inverse = execute_op(
                op,
                &mut portal_buffer,
                &mut commands,
                &mut object_registry,
                &definitions,
                &world_config,
                &editor_camera,
                &asset_server,
                &objects,
            );
            undo_stack.undo_ops.push(inverse);
            editor_state.dirty = true;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_op(
    op: UndoOp,
    portal_buffer: &mut EditorPortalBuffer,
    commands: &mut Commands,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    editor_camera: &EditorCamera,
    asset_server: &AssetServer,
    objects: &Query<(Entity, &OverworldObject, &SpaceResident, &TilePosition)>,
) -> UndoOp {
    match op {
        UndoOp::Despawn { object_id } => {
            if let Some((entity, obj, resident, tile)) =
                objects.iter().find(|(_, o, _, _)| o.object_id == object_id)
            {
                let type_id = object_registry
                    .type_id(obj.object_id)
                    .unwrap_or(&obj.definition_id)
                    .to_owned();
                let properties = object_registry
                    .properties(obj.object_id)
                    .cloned()
                    .unwrap_or_default();
                let space_id = resident.space_id;
                let tile = *tile;
                commands.entity(entity).despawn();
                UndoOp::Spawn {
                    type_id,
                    space_id,
                    tile,
                    properties,
                }
            } else {
                UndoOp::Despawn { object_id }
            }
        }
        UndoOp::Spawn {
            type_id,
            space_id,
            tile,
            properties,
        } => {
            let new_id = object_registry.allocate_runtime_id(type_id.clone());
            let entity = spawn_overworld_object(
                commands,
                definitions,
                new_id,
                &type_id,
                None,
                space_id,
                tile,
                None,
            );
            if !properties.is_empty() {
                object_registry.set_properties(new_id, properties.clone());
            }
            if let Some(def) = definitions.get(&type_id) {
                insert_editor_visuals_pub(
                    &mut commands.entity(entity),
                    asset_server,
                    def,
                    world_config,
                    tile,
                    editor_camera,
                );
            }
            UndoOp::Despawn { object_id: new_id }
        }
        UndoOp::RemovePortal { index } => {
            if index < portal_buffer.portals.len() {
                let portal = portal_buffer.portals.remove(index);
                UndoOp::AddPortal { portal }
            } else {
                UndoOp::RemovePortal { index }
            }
        }
        UndoOp::AddPortal { portal } => {
            let index = portal_buffer.portals.len();
            portal_buffer.portals.push(portal.clone());
            UndoOp::RemovePortal { index }
        }
    }
}
