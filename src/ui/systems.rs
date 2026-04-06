use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::player::components::{Player, VitalStats};
use crate::ui::components::{
    CloseContainerButton, ContainerSlot, ContainerSlotImage, DragPreviewLabel, DragPreviewRoot,
    HealthFill, ManaFill, OpenContainerTitle,
};
use crate::ui::resources::{DragSource, DragState, InventoryState, OpenContainerState};
use crate::world::components::{Collectible, Collider, Container, OverworldObject, TilePosition};
use crate::world::map_layout::MapLayout;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::setup::spawn_overworld_object_instance;
use crate::world::WorldConfig;

pub fn manage_open_containers(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    mut open_container_state: ResMut<OpenContainerState>,
    player_query: Query<&TilePosition, With<Player>>,
    container_query: Query<(Entity, &TilePosition, &OverworldObject), With<Container>>,
    close_button_query: Query<(&ComputedNode, &UiGlobalTransform), With<CloseContainerButton>>,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Ok(player_position) = player_query.single() else {
        return;
    };

    if let Some(entity) = open_container_state.entity {
        let should_close = container_query
            .get(entity)
            .map(|(_, tile_position, _)| !is_near_player(player_position, tile_position))
            .unwrap_or(true);

        if should_close {
            open_container_state.entity = None;
        }
    }

    if mouse_input.just_pressed(MouseButton::Left)
        && open_container_state.entity.is_some()
        && is_cursor_over_close_button(cursor_position, &close_button_query)
    {
        open_container_state.entity = None;
        return;
    }

    if !mouse_input.just_pressed(MouseButton::Right) {
        return;
    }

    let target_tile = cursor_to_tile(window, cursor_position, player_position, &world_config);

    for (entity, tile_position, _) in &container_query {
        if *tile_position != target_tile {
            continue;
        }

        if !is_near_player(player_position, tile_position) {
            continue;
        }

        open_container_state.entity = Some(entity);
        break;
    }
}

pub fn sync_vital_bars(
    player_query: Query<&VitalStats, With<Player>>,
    mut health_query: Query<&mut Node, With<HealthFill>>,
    mut mana_query: Query<&mut Node, (With<ManaFill>, Without<HealthFill>)>,
) {
    let Ok(vital_stats) = player_query.single() else {
        return;
    };

    let health_ratio = normalized_ratio(vital_stats.health, vital_stats.max_health);
    let mana_ratio = normalized_ratio(vital_stats.mana, vital_stats.max_mana);

    for mut node in &mut health_query {
        node.width = percent(health_ratio * 100.0);
    }

    for mut node in &mut mana_query {
        node.width = percent(mana_ratio * 100.0);
    }
}

pub fn sync_active_container_slots(
    inventory_state: Res<InventoryState>,
    open_container_state: Res<OpenContainerState>,
    map_layout: Res<MapLayout>,
    container_query: Query<(&Container, &OverworldObject)>,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    mut title_query: Query<&mut Text, With<OpenContainerTitle>>,
    mut close_button_query: Query<&mut Visibility, With<CloseContainerButton>>,
    mut image_query: Query<
        (&ContainerSlotImage, &mut ImageNode, &mut Visibility),
        (Without<ContainerSlot>, Without<CloseContainerButton>),
    >,
) {
    let Ok(mut title_text) = title_query.single_mut() else {
        return;
    };
    let Ok(mut close_visibility) = close_button_query.single_mut() else {
        return;
    };

    let (title, active_slots, show_close) = if let Some(entity) = open_container_state.entity {
        if let Ok((container, object)) = container_query.get(entity) {
            let title = definitions
                .get(&object.definition_id)
                .map(|definition| definition.name.clone())
                .unwrap_or_else(|| "Container".to_owned());
            (title, Some(&container.slots), true)
        } else {
            (
                "Backpack".to_owned(),
                Some(&inventory_state.backpack_slots),
                false,
            )
        }
    } else {
        (
            "Backpack".to_owned(),
            Some(&inventory_state.backpack_slots),
            false,
        )
    };

    title_text.0 = title;
    *close_visibility = if show_close {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };

    let Some(active_slots) = active_slots else {
        return;
    };

    for (slot, mut image_node, mut visibility) in &mut image_query {
        let Some(object_id) = active_slots.get(slot.index).and_then(|item| *item) else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(type_id) = map_layout.object_type_id(object_id) else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(definition) = definitions.get(type_id) else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(sprite_path) = &definition.render.sprite_path else {
            *visibility = Visibility::Hidden;
            continue;
        };

        image_node.image = asset_server.load(sprite_path);
        *visibility = Visibility::Visible;
    }
}

pub fn handle_collectible_dragging(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    map_layout: Res<MapLayout>,
    definitions: Res<OverworldObjectDefinitions>,
    mut inventory_state: ResMut<InventoryState>,
    open_container_state: Res<OpenContainerState>,
    mut drag_state: ResMut<DragState>,
    player_query: Query<&TilePosition, With<Player>>,
    collider_query: Query<&TilePosition, (With<Collider>, Without<Player>)>,
    collectible_query: Query<(Entity, &TilePosition, &OverworldObject), With<Collectible>>,
    slot_query: Query<(&ContainerSlot, &ComputedNode, &UiGlobalTransform), With<Button>>,
    mut container_query: Query<&mut Container>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Ok(player_position) = player_query.single() else {
        return;
    };
    let hovered_slot_index = hovered_slot_index(cursor_position, &slot_query);

    if mouse_input.just_pressed(MouseButton::Left) && drag_state.source.is_none() {
        if let Some(slot_index) = hovered_slot_index {
            if let Some(object_id) = take_item_from_active_container(
                &mut inventory_state,
                &mut container_query,
                open_container_state.entity,
                slot_index,
            ) {
                drag_state.source = Some(match open_container_state.entity {
                    Some(entity) => DragSource::OpenContainer(entity, slot_index),
                    None => DragSource::Backpack(slot_index),
                });
                drag_state.object_id = Some(object_id);
                drag_state.world_origin = None;
                return;
            }
        }

        let target_tile = cursor_to_tile(window, cursor_position, player_position, &world_config);

        for (entity, tile_position, object) in &collectible_query {
            if *tile_position != target_tile {
                continue;
            }

            if !is_near_player(player_position, tile_position) {
                continue;
            }

            drag_state.source = Some(DragSource::World(entity));
            drag_state.object_id = Some(object.object_id);
            drag_state.world_origin = Some(*tile_position);
            break;
        }
    }

    if !mouse_input.just_released(MouseButton::Left) || drag_state.source.is_none() {
        return;
    }

    let target_tile = cursor_to_tile(window, cursor_position, player_position, &world_config);
    let drag_source = drag_state.source.take();
    let Some(object_id) = drag_state.object_id.take() else {
        drag_state.world_origin = None;
        return;
    };
    let world_origin = drag_state.world_origin.take();

    match drag_source {
        Some(DragSource::World(item_entity)) => {
            if let Some(slot_index) = hovered_slot_index {
                if place_item_in_active_container(
                    &mut inventory_state,
                    &mut container_query,
                    open_container_state.entity,
                    slot_index,
                    object_id,
                ) {
                    commands.entity(item_entity).despawn();
                    return;
                }
            }

            if let Some(origin) = world_origin {
                if is_valid_world_drop(
                    target_tile,
                    Some(origin),
                    player_position,
                    item_entity,
                    &collider_query,
                    &collectible_query,
                    &world_config,
                ) {
                    commands.entity(item_entity).insert(target_tile);
                }
            }
        }
        Some(DragSource::Backpack(source_slot)) => {
            if let Some(slot_index) = hovered_slot_index {
                if open_container_state.entity.is_none() && slot_index == source_slot {
                    restore_backpack_slot(&mut inventory_state, source_slot, object_id);
                    return;
                }

                if place_item_in_active_container(
                    &mut inventory_state,
                    &mut container_query,
                    open_container_state.entity,
                    slot_index,
                    object_id,
                ) {
                    return;
                }
            }

            if let Some(world_drop_tile) = find_nearest_valid_world_drop_tile(
                target_tile,
                None,
                player_position,
                Entity::PLACEHOLDER,
                &collider_query,
                &collectible_query,
                &world_config,
            ) {
                if let Some(object) = map_layout.get_object(object_id) {
                    spawn_overworld_object_instance(
                        &mut commands,
                        &asset_server,
                        &map_layout,
                        &definitions,
                        &world_config,
                        object,
                        world_drop_tile,
                    );
                    return;
                }
            }

            restore_backpack_slot(&mut inventory_state, source_slot, object_id);
        }
        Some(DragSource::OpenContainer(entity, source_slot)) => {
            if let Some(slot_index) = hovered_slot_index {
                if open_container_state.entity == Some(entity) && slot_index == source_slot {
                    restore_container_slot(&mut container_query, entity, source_slot, object_id);
                    return;
                }

                if place_item_in_active_container(
                    &mut inventory_state,
                    &mut container_query,
                    open_container_state.entity,
                    slot_index,
                    object_id,
                ) {
                    return;
                }
            }

            if let Some(world_drop_tile) = find_nearest_valid_world_drop_tile(
                target_tile,
                None,
                player_position,
                Entity::PLACEHOLDER,
                &collider_query,
                &collectible_query,
                &world_config,
            ) {
                if let Some(object) = map_layout.get_object(object_id) {
                    spawn_overworld_object_instance(
                        &mut commands,
                        &asset_server,
                        &map_layout,
                        &definitions,
                        &world_config,
                        object,
                        world_drop_tile,
                    );
                    return;
                }
            }

            restore_container_slot(&mut container_query, entity, source_slot, object_id);
        }
        None => {}
    }
}

pub fn sync_drag_preview(
    drag_state: Res<DragState>,
    map_layout: Res<MapLayout>,
    definitions: Res<OverworldObjectDefinitions>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut preview_query: Query<(&mut Node, &mut Visibility), With<DragPreviewRoot>>,
    mut label_query: Query<&mut Text, (With<DragPreviewLabel>, Without<DragPreviewRoot>)>,
) {
    let Ok((mut preview_node, mut visibility)) = preview_query.single_mut() else {
        return;
    };
    let Ok(mut label) = label_query.single_mut() else {
        return;
    };

    let Some(object_id) = drag_state.object_id else {
        *visibility = Visibility::Hidden;
        label.0.clear();
        return;
    };

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        *visibility = Visibility::Hidden;
        label.0.clear();
        return;
    };

    *visibility = Visibility::Visible;
    preview_node.left = px(cursor_position.x + 14.0);
    preview_node.top = px(cursor_position.y + 14.0);

    if let Some(type_id) = map_layout.object_type_id(object_id) {
        if let Some(definition) = definitions.get(type_id) {
            label.0 = definition.name.clone();
            return;
        }
    }

    label.0 = object_id.to_string();
}

fn normalized_ratio(current: f32, maximum: f32) -> f32 {
    if maximum <= 0.0 {
        return 0.0;
    }

    (current / maximum).clamp(0.0, 1.0)
}

fn is_cursor_over_close_button(
    cursor_position: Vec2,
    close_button_query: &Query<(&ComputedNode, &UiGlobalTransform), With<CloseContainerButton>>,
) -> bool {
    let Ok((computed_node, global_transform)) = close_button_query.single() else {
        return false;
    };

    point_in_ui_node(cursor_position, computed_node, global_transform)
}

fn hovered_slot_index(
    cursor_position: Vec2,
    slot_query: &Query<(&ContainerSlot, &ComputedNode, &UiGlobalTransform), With<Button>>,
) -> Option<usize> {
    slot_query
        .iter()
        .find_map(|(slot, computed_node, global_transform)| {
            point_in_ui_node(cursor_position, computed_node, global_transform).then_some(slot.index)
        })
}

fn point_in_ui_node(
    cursor_position: Vec2,
    computed_node: &ComputedNode,
    global_transform: &UiGlobalTransform,
) -> bool {
    let Some(local_point) = global_transform
        .try_inverse()
        .map(|transform| transform.transform_point2(cursor_position) + 0.5 * computed_node.size())
    else {
        return false;
    };

    let size = computed_node.size();
    local_point.x >= 0.0
        && local_point.y >= 0.0
        && local_point.x <= size.x
        && local_point.y <= size.y
}

fn take_item_from_active_container(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    open_container_entity: Option<Entity>,
    slot_index: usize,
) -> Option<u64> {
    if let Some(entity) = open_container_entity {
        return container_query
            .get_mut(entity)
            .ok()
            .and_then(|mut container| container.slots.get_mut(slot_index)?.take());
    }

    inventory_state.backpack_slots.get_mut(slot_index)?.take()
}

fn place_item_in_active_container(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    open_container_entity: Option<Entity>,
    slot_index: usize,
    object_id: u64,
) -> bool {
    if let Some(entity) = open_container_entity {
        let Ok(mut container) = container_query.get_mut(entity) else {
            return false;
        };
        let Some(slot) = container.slots.get_mut(slot_index) else {
            return false;
        };
        if slot.is_some() {
            return false;
        }
        *slot = Some(object_id);
        return true;
    }

    let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) else {
        return false;
    };
    if slot.is_some() {
        return false;
    }
    *slot = Some(object_id);
    true
}

fn restore_backpack_slot(inventory_state: &mut InventoryState, slot_index: usize, object_id: u64) {
    if let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) {
        *slot = Some(object_id);
    }
}

fn restore_container_slot(
    container_query: &mut Query<&mut Container>,
    entity: Entity,
    slot_index: usize,
    object_id: u64,
) {
    if let Ok(mut container) = container_query.get_mut(entity) {
        if let Some(slot) = container.slots.get_mut(slot_index) {
            *slot = Some(object_id);
        }
    }
}

fn cursor_to_tile(
    window: &Window,
    cursor_position: Vec2,
    player_position: &TilePosition,
    world_config: &WorldConfig,
) -> TilePosition {
    let window_center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let cursor_offset = cursor_position - window_center;
    let tile_offset_x = (cursor_offset.x / world_config.tile_size).round() as i32;
    let tile_offset_y = (-cursor_offset.y / world_config.tile_size).round() as i32;

    TilePosition::new(
        player_position.x + tile_offset_x,
        player_position.y + tile_offset_y,
    )
}

fn is_near_player(player_position: &TilePosition, target_position: &TilePosition) -> bool {
    let delta_x = (player_position.x - target_position.x).abs();
    let delta_y = (player_position.y - target_position.y).abs();

    delta_x <= 1 && delta_y <= 1
}

fn is_valid_world_drop(
    target_tile: TilePosition,
    source_world_tile: Option<TilePosition>,
    player_position: &TilePosition,
    dragged_entity: Entity,
    collider_query: &Query<&TilePosition, (With<Collider>, Without<Player>)>,
    collectible_query: &Query<(Entity, &TilePosition, &OverworldObject), With<Collectible>>,
    world_config: &WorldConfig,
) -> bool {
    if target_tile.x < 0
        || target_tile.y < 0
        || target_tile.x >= world_config.map_width
        || target_tile.y >= world_config.map_height
    {
        return false;
    }

    if !is_near_player(player_position, &target_tile) {
        return false;
    }

    if let Some(source_tile) = source_world_tile {
        let delta_x = (source_tile.x - target_tile.x).abs();
        let delta_y = (source_tile.y - target_tile.y).abs();
        if delta_x > 1 || delta_y > 1 {
            return false;
        }
    }

    if collider_query.iter().any(|tile| *tile == target_tile) {
        return false;
    }

    !collectible_query
        .iter()
        .any(|(entity, tile, _)| entity != dragged_entity && *tile == target_tile)
}

fn find_nearest_valid_world_drop_tile(
    requested_tile: TilePosition,
    source_world_tile: Option<TilePosition>,
    player_position: &TilePosition,
    dragged_entity: Entity,
    collider_query: &Query<&TilePosition, (With<Collider>, Without<Player>)>,
    collectible_query: &Query<(Entity, &TilePosition, &OverworldObject), With<Collectible>>,
    world_config: &WorldConfig,
) -> Option<TilePosition> {
    let mut candidates = Vec::new();

    for y in (player_position.y - 1)..=(player_position.y + 1) {
        for x in (player_position.x - 1)..=(player_position.x + 1) {
            let tile = TilePosition::new(x, y);
            let distance = (requested_tile.x - x).abs() + (requested_tile.y - y).abs();
            candidates.push((distance, tile));
        }
    }

    candidates.sort_by_key(|(distance, _)| *distance);

    for (_, candidate) in candidates {
        if is_valid_world_drop(
            candidate,
            source_world_tile,
            player_position,
            dragged_entity,
            collider_query,
            collectible_query,
            world_config,
        ) {
            return Some(candidate);
        }
    }

    None
}
