use bevy::ecs::query::QueryFilter;
use bevy::log::{info, warn};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::combat::components::CombatTarget;
use crate::magic::resources::{SpellDefinition, SpellDefinitions, SpellTargeting};
use crate::npc::components::Npc;
use crate::player::components::{DerivedStats, Player, VitalStats};
use crate::scripting::resources::PythonConsoleState;
use crate::ui::components::{
    ChatLogText, ClearCombatTargetButton, CloseContainerButton, ContainerSlotButton,
    ContainerSlotImage, ContextMenuAttackButton, ContextMenuInspectButton, ContextMenuOpenButton,
    ContextMenuRoot, ContextMenuUseButton, ContextMenuUseOnButton, CurrentCombatTargetLabel,
    DragPreviewLabel, DragPreviewRoot, EquipmentSlotButton, EquipmentSlotImage, HealthFill,
    HealthLabel, ItemSlotButton, ItemSlotImage, ItemSlotKind, ManaFill, ManaLabel,
    OpenContainerTitle,
};
use crate::ui::resources::{
    ChatLogState, ContextMenuState, ContextMenuTarget, CursorMode, CursorState, DragSource,
    DragState, InventoryState, OpenContainerState, SpellTargetingState, UseOnState,
};
use crate::world::components::{Collider, Container, Movable, OverworldObject, TilePosition};
use crate::world::object_definitions::{
    EquipmentSlot, OverworldObjectDefinition, OverworldObjectDefinitions,
};
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::spawn_overworld_object;
use crate::world::WorldConfig;

pub fn toggle_cursor_mode(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    console_state: Option<Res<PythonConsoleState>>,
    use_on_state: Res<UseOnState>,
    spell_targeting_state: Res<SpellTargetingState>,
    mut cursor_state: ResMut<CursorState>,
) {
    if console_state.as_ref().is_some_and(|state| state.is_open) {
        return;
    }
    if use_on_state.source.is_some() {
        return;
    }
    if spell_targeting_state.source.is_some() {
        return;
    }

    if keyboard_input.just_pressed(KeyCode::KeyU) {
        cursor_state.mode = match cursor_state.mode {
            CursorMode::Default => CursorMode::UseOn,
            CursorMode::UseOn => CursorMode::Default,
            CursorMode::SpellTarget => CursorMode::SpellTarget,
        };
    }
}

pub fn setup_native_custom_cursor(
    asset_server: Res<AssetServer>,
    window_entity: Single<Entity, With<PrimaryWindow>>,
    mut commands: Commands,
) {
    commands
        .entity(*window_entity)
        .insert(cursor_icon_for_mode(CursorMode::Default, &asset_server));
}

pub fn sync_native_custom_cursor(
    cursor_state: Res<CursorState>,
    asset_server: Res<AssetServer>,
    mut window_query: Query<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if !cursor_state.is_changed() {
        return;
    }

    let Ok(mut cursor_icon) = window_query.single_mut() else {
        return;
    };

    *cursor_icon = cursor_icon_for_mode(cursor_state.mode, &asset_server);
}

fn cursor_icon_for_mode(cursor_mode: CursorMode, asset_server: &AssetServer) -> CursorIcon {
    let asset_path = match cursor_mode {
        CursorMode::Default => "cursors/default_cursor.png",
        CursorMode::UseOn => "cursors/use_on_cursor.png",
        CursorMode::SpellTarget => "cursors/spell_target_cursor.png",
    };

    CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
        handle: asset_server.load(asset_path),
        texture_atlas: None,
        flip_x: false,
        flip_y: false,
        rect: None,
        hotspot: (0, 0),
    }))
}

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

    let _ = world_config;
}

pub fn sync_current_combat_target(
    player_query: Query<&CombatTarget, With<Player>>,
    object_query: Query<&OverworldObject>,
    object_registry: Res<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    mut label_query: Query<&mut Text, With<CurrentCombatTargetLabel>>,
) {
    let Ok(mut label) = label_query.single_mut() else {
        return;
    };

    let text = if let Ok(combat_target) = player_query.single() {
        if let Ok(object) = object_query.get(combat_target.entity) {
            let name = object_registry
                .display_name(object.object_id, &definitions, &spell_definitions)
                .unwrap_or_else(|| object.definition_id.clone());
            format!("Target: {name}")
        } else {
            "Target: none".to_owned()
        }
    } else {
        "Target: none".to_owned()
    };

    label.0 = text;
}

pub fn sync_clear_combat_target_button(
    player_query: Query<&CombatTarget, With<Player>>,
    mut button_query: Query<&mut Visibility, With<ClearCombatTargetButton>>,
) {
    let Ok(mut visibility) = button_query.single_mut() else {
        return;
    };

    *visibility = if player_query.single().is_ok() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

pub fn sync_vital_bars(
    player_query: Query<&VitalStats, With<Player>>,
    mut health_query: Query<&mut Node, With<HealthFill>>,
    mut mana_query: Query<&mut Node, (With<ManaFill>, Without<HealthFill>)>,
    mut health_label_query: Query<&mut Text, (With<HealthLabel>, Without<ManaLabel>)>,
    mut mana_label_query: Query<&mut Text, (With<ManaLabel>, Without<HealthLabel>)>,
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

    let health_text = format!(
        "{}/{}",
        vital_stats.health.round() as i32,
        vital_stats.max_health.round() as i32
    );
    let mana_text = format!(
        "{}/{}",
        vital_stats.mana.round() as i32,
        vital_stats.max_mana.round() as i32
    );

    for mut label in &mut health_label_query {
        label.0 = health_text.clone();
    }

    for mut label in &mut mana_label_query {
        label.0 = mana_text.clone();
    }
}

pub fn sync_chat_log(
    chat_log_state: Res<ChatLogState>,
    mut chat_query: Query<&mut Text, With<ChatLogText>>,
) {
    let Ok(mut chat_text) = chat_query.single_mut() else {
        return;
    };

    chat_text.0 = chat_log_state.lines.join("\n");
}

pub fn sync_context_menu_root(
    context_menu_state: Res<ContextMenuState>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut root_query: Query<(&mut Node, &mut Visibility, &ComputedNode), With<ContextMenuRoot>>,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Ok((mut root_node, mut root_visibility, computed_node)) = root_query.single_mut() else {
        return;
    };

    *root_visibility = if context_menu_state.is_visible() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };

    let menu_size = computed_node.size();
    let max_left = (window.width() - menu_size.x).max(0.0);
    let max_top = (window.height() - menu_size.y).max(0.0);
    let clamped_left = context_menu_state.position.x.clamp(0.0, max_left);
    let clamped_top = context_menu_state.position.y.clamp(0.0, max_top);

    root_node.left = px(clamped_left);
    root_node.top = px(clamped_top);
}

pub fn sync_context_menu_open_button(
    context_menu_state: Res<ContextMenuState>,
    mut open_button_query: Query<&mut Visibility, With<ContextMenuOpenButton>>,
) {
    let Ok(mut open_visibility) = open_button_query.single_mut() else {
        return;
    };

    *open_visibility = if context_menu_state.can_open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

pub fn sync_context_menu_attack_button(
    context_menu_state: Res<ContextMenuState>,
    mut attack_button_query: Query<&mut Visibility, With<ContextMenuAttackButton>>,
) {
    let Ok(mut attack_visibility) = attack_button_query.single_mut() else {
        return;
    };

    *attack_visibility = if context_menu_state.can_attack {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

pub fn sync_context_menu_use_button(
    context_menu_state: Res<ContextMenuState>,
    mut use_button_query: Query<&mut Visibility, With<ContextMenuUseButton>>,
) {
    let Ok(mut use_visibility) = use_button_query.single_mut() else {
        return;
    };

    *use_visibility = if context_menu_state.can_use {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

pub fn sync_context_menu_use_on_button(
    context_menu_state: Res<ContextMenuState>,
    mut use_on_button_query: Query<&mut Visibility, With<ContextMenuUseOnButton>>,
) {
    let Ok(mut use_on_visibility) = use_on_button_query.single_mut() else {
        return;
    };

    *use_on_visibility = if context_menu_state.can_use_on {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

pub fn handle_context_menu_actions(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    static_resources: (
        Res<ObjectRegistry>,
        Res<OverworldObjectDefinitions>,
        Res<SpellDefinitions>,
    ),
    ui_state: (
        ResMut<ChatLogState>,
        ResMut<ContextMenuState>,
        ResMut<OpenContainerState>,
        ResMut<CursorState>,
        ResMut<UseOnState>,
        ResMut<SpellTargetingState>,
    ),
    mut menu_queries: ParamSet<(
        Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<ContextMenuAttackButton>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<ContextMenuInspectButton>>,
        Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<ContextMenuOpenButton>>,
        Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<ContextMenuUseButton>>,
        Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<ContextMenuUseOnButton>>,
    )>,
    mut inventory_state: ResMut<InventoryState>,
    mut container_query: Query<&mut Container>,
    player_entity_query: Query<Entity, With<Player>>,
    mut player_query: Query<&mut VitalStats, With<Player>>,
    mut commands: Commands,
) {
    let (object_registry, definitions, spell_definitions) = static_resources;
    let (
        mut chat_log_state,
        mut context_menu_state,
        mut open_container_state,
        mut cursor_state,
        mut use_on_state,
        mut spell_targeting_state,
    ) = ui_state;
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    if !mouse_input.just_pressed(MouseButton::Left) || !context_menu_state.is_visible() {
        return;
    }

    if is_cursor_over_visible_button(cursor_position, &menu_queries.p0()) {
        if let Some(ContextMenuTarget::World(target_entity, object_id)) = context_menu_state.target
        {
            if let Ok(player_entity) = player_entity_query.single() {
                commands.entity(player_entity).insert(CombatTarget {
                    entity: target_entity,
                });

                if let Some(target_name) = object_name(
                    object_id,
                    &object_registry,
                    &definitions,
                    &spell_definitions,
                ) {
                    chat_log_state.push_narrator(format!("Targeting {target_name}."));
                }
            }
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_button(cursor_position, &menu_queries.p1()) {
        if let Some(target) = context_menu_state.target {
            inspect_context_target(
                target,
                &object_registry,
                &definitions,
                &spell_definitions,
                &mut chat_log_state,
            );
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_visible_button(cursor_position, &menu_queries.p3()) {
        if let Some(target) = context_menu_state.target {
            handle_use_action(
                target,
                &mut inventory_state,
                &mut container_query,
                open_container_state.entity,
                &object_registry,
                &definitions,
                &spell_definitions,
                &mut player_query,
                &mut chat_log_state,
                &mut cursor_state,
                &mut spell_targeting_state,
                &mut commands,
            );
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_visible_button(cursor_position, &menu_queries.p4()) {
        if let Some(target) = context_menu_state.target {
            let object_id = context_target_object_id(target);
            if object_is_usable(object_id, &object_registry, &definitions) {
                use_on_state.source = Some(target);
                cursor_state.mode = CursorMode::UseOn;
            }
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_visible_button(cursor_position, &menu_queries.p2()) {
        if let Some(ContextMenuTarget::World(entity, _)) = context_menu_state.target {
            open_container_state.entity = Some(entity);
        }
        context_menu_state.hide();
        return;
    }

    context_menu_state.hide();
}

pub fn handle_clear_combat_target(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut player_query: Query<Entity, (With<Player>, With<CombatTarget>)>,
    button_query: Query<
        (&ComputedNode, &UiGlobalTransform, &Visibility),
        With<ClearCombatTargetButton>,
    >,
    mut commands: Commands,
) {
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    if !is_cursor_over_visible_button(cursor_position, &button_query) {
        return;
    }

    let Ok(player_entity) = player_query.single_mut() else {
        return;
    };

    commands.entity(player_entity).remove::<CombatTarget>();
}

pub fn handle_use_on_targeting(
    mouse_input: Res<ButtonInput<MouseButton>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    state_resources: (
        Res<ContextMenuState>,
        Res<ObjectRegistry>,
        Res<OverworldObjectDefinitions>,
        Res<SpellDefinitions>,
        Res<OpenContainerState>,
    ),
    mut inventory_state: ResMut<InventoryState>,
    mut container_query: Query<&mut Container>,
    mut player_queries: ParamSet<(
        Query<&mut VitalStats, With<Player>>,
        Query<&TilePosition, With<Player>>,
    )>,
    object_query: Query<(Entity, &TilePosition, &OverworldObject)>,
    mut chat_log_state: ResMut<ChatLogState>,
    mut cursor_state: ResMut<CursorState>,
    mut use_on_state: ResMut<UseOnState>,
    mut commands: Commands,
) {
    let (context_menu_state, object_registry, definitions, spell_definitions, open_container_state) =
        state_resources;
    let Some(source_target) = use_on_state.source else {
        return;
    };

    if keyboard_input.just_pressed(KeyCode::Escape) || mouse_input.just_pressed(MouseButton::Right)
    {
        use_on_state.source = None;
        cursor_state.mode = CursorMode::Default;
        return;
    }

    if context_menu_state.is_visible() || !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let player_position_query = player_queries.p1();
    let Ok(player_position) = player_position_query.single() else {
        return;
    };

    let target_tile = cursor_to_tile(window, cursor_position, player_position, &world_config);

    if target_tile == *player_position {
        use_on_player_target(
            source_target,
            &mut inventory_state,
            &mut container_query,
            open_container_state.entity,
            &object_registry,
            &definitions,
            &spell_definitions,
            &mut player_queries.p0(),
            &mut chat_log_state,
            &mut commands,
        );
        use_on_state.source = None;
        cursor_state.mode = CursorMode::Default;
        return;
    }

    for (_, tile_position, object) in &object_query {
        if *tile_position != target_tile {
            continue;
        }
        if !is_near_player(player_position, tile_position) {
            continue;
        }

        use_on_world_target(
            source_target,
            object.object_id,
            &object_registry,
            &definitions,
            &spell_definitions,
            &mut chat_log_state,
        );
        use_on_state.source = None;
        cursor_state.mode = CursorMode::Default;
        return;
    }
}

pub fn handle_spell_targeting(
    mouse_input: Res<ButtonInput<MouseButton>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    static_resources: (
        Res<ContextMenuState>,
        Res<ObjectRegistry>,
        Res<OverworldObjectDefinitions>,
        Res<SpellDefinitions>,
        Res<OpenContainerState>,
    ),
    mut inventory_state: ResMut<InventoryState>,
    mut container_query: Query<&mut Container>,
    mut player_queries: ParamSet<(
        Query<(&mut VitalStats, &TilePosition), With<Player>>,
        Query<(Entity, &TilePosition, &OverworldObject), (With<Npc>, Without<Player>)>,
        Query<(&mut VitalStats, &OverworldObject), (With<Npc>, Without<Player>)>,
    )>,
    mut chat_log_state: ResMut<ChatLogState>,
    mut cursor_state: ResMut<CursorState>,
    mut spell_targeting_state: ResMut<SpellTargetingState>,
    mut commands: Commands,
) {
    let (context_menu_state, object_registry, definitions, spell_definitions, open_container_state) =
        static_resources;
    let (Some(source_target), Some(spell_id)) = (
        spell_targeting_state.source,
        spell_targeting_state.spell_id.as_deref(),
    ) else {
        return;
    };

    if keyboard_input.just_pressed(KeyCode::Escape) || mouse_input.just_pressed(MouseButton::Right)
    {
        spell_targeting_state.source = None;
        spell_targeting_state.spell_id = None;
        cursor_state.mode = CursorMode::Default;
        return;
    }

    if context_menu_state.is_visible() || !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(spell) = spell_definitions.get(spell_id) else {
        spell_targeting_state.source = None;
        spell_targeting_state.spell_id = None;
        cursor_state.mode = CursorMode::Default;
        return;
    };

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    let target_tile = {
        let player_query = player_queries.p0();
        let Ok((_, player_position)) = player_query.single() else {
            return;
        };
        cursor_to_tile(window, cursor_position, player_position, &world_config)
    };

    let mut selected_target = None;
    {
        let npc_query = player_queries.p1();
        for (target_entity, target_position, target_object) in &npc_query {
            if *target_position != target_tile {
                continue;
            }

            selected_target = Some((target_entity, *target_position, target_object.object_id));
            break;
        }
    }

    let Some((target_entity, target_position, target_object_id)) = selected_target else {
        chat_log_state.push_narrator(format!("{} needs a valid target.", spell.name));
        return;
    };

    let player_position = {
        let mut player_query = player_queries.p0();
        let Ok((_, player_position)) = player_query.single_mut() else {
            return;
        };
        *player_position
    };

    if chebyshev_distance_tiles(player_position, target_position) > spell.range_tiles.max(1) {
        let target_name = object_registry
            .display_name(target_object_id, &definitions, &spell_definitions)
            .unwrap_or_else(|| target_object_id.to_string());
        chat_log_state.push_narrator(format!(
            "{} is out of range for {}.",
            target_name, spell.name
        ));
        return;
    }

    {
        let mut player_query = player_queries.p0();
        let Ok((mut player_vitals, _)) = player_query.single_mut() else {
            return;
        };
        if player_vitals.mana < spell.mana_cost {
            chat_log_state.push_narrator(format!("Not enough mana to cast {}.", spell.name));
            return;
        }
        player_vitals.mana = (player_vitals.mana - spell.mana_cost).max(0.0);
    }

    {
        let mut npc_query = player_queries.p2();
        let Ok((mut target_vitals, target_object)) = npc_query.get_mut(target_entity) else {
            return;
        };
        let target_name = object_registry
            .display_name(target_object.object_id, &definitions, &spell_definitions)
            .unwrap_or_else(|| target_object.definition_id.clone());

        apply_spell_effects(spell, &mut target_vitals);
        consume_source_target(
            source_target,
            &mut inventory_state,
            &mut container_query,
            open_container_state.entity,
            &mut commands,
        );
        chat_log_state.push_line(format!("[Player]: \"{}\"", spell.incantation));
        chat_log_state.push_narrator(format!("Cast {} on {}.", spell.name, target_name));

        if target_vitals.health <= 0.0 {
            commands.entity(target_entity).despawn();
            chat_log_state.push_line(format!("[{target_name} dies]"));
        }
    }

    spell_targeting_state.source = None;
    spell_targeting_state.spell_id = None;
    cursor_state.mode = CursorMode::Default;
}

pub fn handle_context_menu_opening(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    object_registry: Res<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    inventory_state: Res<InventoryState>,
    mut context_menu_state: ResMut<ContextMenuState>,
    open_container_state: Res<OpenContainerState>,
    mut use_on_state: ResMut<UseOnState>,
    mut spell_targeting_state: ResMut<SpellTargetingState>,
    mut cursor_state: ResMut<CursorState>,
    player_query: Query<&TilePosition, With<Player>>,
    object_query: Query<(
        Entity,
        &TilePosition,
        &OverworldObject,
        Has<Container>,
        Has<Npc>,
    )>,
    mut slot_queries: ParamSet<(
        Query<
            (
                &ItemSlotButton,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            (With<Button>, With<EquipmentSlotButton>),
        >,
        Query<
            (
                &ItemSlotButton,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            (With<Button>, With<ContainerSlotButton>),
        >,
        Query<
            (
                &ItemSlotImage,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            With<EquipmentSlotImage>,
        >,
        Query<
            (
                &ItemSlotImage,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            With<ContainerSlotImage>,
        >,
    )>,
    container_query: Query<&Container>,
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

    if !mouse_input.just_pressed(MouseButton::Right) {
        return;
    }

    if spell_targeting_state.source.is_some() {
        spell_targeting_state.source = None;
        spell_targeting_state.spell_id = None;
        cursor_state.mode = CursorMode::Default;
        context_menu_state.hide();
        return;
    }

    if use_on_state.source.is_some() {
        use_on_state.source = None;
        cursor_state.mode = CursorMode::Default;
        context_menu_state.hide();
        return;
    }

    let hovered_slot = hovered_slot_kind_from_ui(cursor_position, &mut slot_queries);
    info!(
        "context_open_attempt cursor=({:.1}, {:.1}) open_container={:?} hovered_slot={hovered_slot:?}",
        cursor_position.x,
        cursor_position.y,
        open_container_state.entity
    );
    if let Some(slot_kind) = hovered_slot {
        let slot_object_id = object_id_in_slot_kind(
            &inventory_state,
            &container_query,
            open_container_state.entity,
            slot_kind,
        );
        info!(
            "context_open_slot slot={slot_kind:?} resolved_object_id={slot_object_id:?} open_container={:?}",
            open_container_state.entity
        );
        if let Some(object_id) = slot_object_id {
            let can_use = object_is_usable(object_id, &object_registry, &definitions);
            context_menu_state.show(
                cursor_position,
                ContextMenuTarget::Slot(slot_kind, object_id),
                false,
                can_use,
                can_use_on(
                    object_id,
                    &object_registry,
                    &definitions,
                    &spell_definitions,
                ),
                false,
            );
            info!(
                "context_open_slot_success slot={slot_kind:?} object_id={object_id} can_use={can_use}"
            );
            return;
        }
    }

    let target_tile = cursor_to_tile(window, cursor_position, player_position, &world_config);
    info!(
        "context_open_world_probe target_tile=({}, {})",
        target_tile.x, target_tile.y
    );
    for (entity, tile_position, object, has_container, has_npc) in &object_query {
        if *tile_position != target_tile {
            continue;
        }
        if !is_near_player(player_position, tile_position) {
            continue;
        }

        let can_use = object_is_usable(object.object_id, &object_registry, &definitions);
        context_menu_state.show(
            cursor_position,
            ContextMenuTarget::World(entity, object.object_id),
            has_container,
            can_use,
            can_use_on(
                object.object_id,
                &object_registry,
                &definitions,
                &spell_definitions,
            ),
            has_npc,
        );
        info!(
            "context_open_world_success entity={entity:?} object_id={} has_container={} can_use={} can_attack={}",
            object.object_id, has_container, can_use, has_npc
        );
        return;
    }

    info!("context_open_no_target");
    context_menu_state.hide();
}

pub fn sync_open_container_title(
    open_container_state: Res<OpenContainerState>,
    object_registry: Res<ObjectRegistry>,
    container_query: Query<(&Container, &OverworldObject)>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    mut title_query: Query<&mut Text, With<OpenContainerTitle>>,
) {
    let Ok(mut title_text) = title_query.single_mut() else {
        return;
    };

    let title = if let Some(entity) = open_container_state.entity {
        if let Ok((_, object)) = container_query.get(entity) {
            object_registry
                .display_name(object.object_id, &definitions, &spell_definitions)
                .unwrap_or_else(|| "Container".to_owned())
        } else {
            "Backpack".to_owned()
        }
    } else {
        "Backpack".to_owned()
    };

    title_text.0 = title;
}

pub fn sync_close_container_button(
    open_container_state: Res<OpenContainerState>,
    mut close_button_query: Query<&mut Visibility, With<CloseContainerButton>>,
) {
    let Ok(mut close_visibility) = close_button_query.single_mut() else {
        return;
    };

    *close_visibility = if open_container_state.entity.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

pub fn sync_item_slot_button_visibility(
    inventory_state: Res<InventoryState>,
    open_container_state: Res<OpenContainerState>,
    derived_stats_query: Query<&DerivedStats, With<Player>>,
    container_query: Query<&Container>,
    mut slot_button_query: Query<(&ItemSlotButton, &mut Visibility), With<ContainerSlotButton>>,
) {
    let Ok(derived_stats) = derived_stats_query.single() else {
        return;
    };

    let active_container_capacity = open_container_state
        .entity
        .and_then(|entity| container_query.get(entity).ok())
        .map(|container| container.slots.len())
        .unwrap_or(inventory_state.backpack_slots.len());
    let occupied_backpack_slots = inventory_state
        .backpack_slots
        .iter()
        .rposition(Option::is_some)
        .map(|index| index + 1)
        .unwrap_or(0);
    let visible_backpack_capacity = if open_container_state.entity.is_some() {
        active_container_capacity
    } else {
        occupied_backpack_slots.max(
            derived_stats
                .storage_slots
                .min(inventory_state.backpack_slots.len()),
        )
    };

    for (slot, mut visibility) in &mut slot_button_query {
        let should_show = match slot.kind {
            ItemSlotKind::ActiveContainer(index) => index < visible_backpack_capacity,
            ItemSlotKind::Equipment(_) => true,
        };

        *visibility = if should_show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

pub fn sync_container_slot_images(
    inventory_state: Res<InventoryState>,
    open_container_state: Res<OpenContainerState>,
    object_registry: Res<ObjectRegistry>,
    container_query: Query<&Container>,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    mut image_query: Query<
        (&ItemSlotImage, &mut ImageNode, &mut Visibility),
        With<ContainerSlotImage>,
    >,
) {
    for (slot, mut image_node, mut visibility) in &mut image_query {
        let object_id = match slot.kind {
            ItemSlotKind::ActiveContainer(index) => active_object_id_in_container_view(
                &inventory_state,
                &container_query,
                open_container_state.entity,
                index,
            ),
            ItemSlotKind::Equipment(_) => None,
        };
        let Some(object_id) = object_id else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(type_id) = object_registry.type_id(object_id) else {
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

pub fn sync_equipment_slot_images(
    inventory_state: Res<InventoryState>,
    object_registry: Res<ObjectRegistry>,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    mut image_query: Query<
        (&ItemSlotImage, &mut ImageNode, &mut Visibility),
        With<EquipmentSlotImage>,
    >,
) {
    for (slot, mut image_node, mut visibility) in &mut image_query {
        let object_id = match slot.kind {
            ItemSlotKind::Equipment(slot) => inventory_state.equipment_item(slot),
            ItemSlotKind::ActiveContainer(_) => None,
        };
        let Some(object_id) = object_id else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(type_id) = object_registry.type_id(object_id) else {
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

pub fn handle_movable_dragging(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    interaction_state: (Res<ContextMenuState>, Res<UseOnState>),
    object_registry: Res<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    mut inventory_state: ResMut<InventoryState>,
    open_container_state: Res<OpenContainerState>,
    mut drag_state: ResMut<DragState>,
    player_query: Query<&TilePosition, With<Player>>,
    collider_query: Query<&TilePosition, (With<Collider>, Without<Player>)>,
    movable_query: Query<(Entity, &TilePosition, &OverworldObject), With<Movable>>,
    mut slot_queries: ParamSet<(
        Query<
            (
                &ItemSlotButton,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            (With<Button>, With<EquipmentSlotButton>),
        >,
        Query<
            (
                &ItemSlotButton,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            (With<Button>, With<ContainerSlotButton>),
        >,
    )>,
    mut container_query: Query<&mut Container>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let (context_menu_state, use_on_state) = interaction_state;
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Ok(player_position) = player_query.single() else {
        return;
    };
    let hovered_slot = {
        let equipment_hovered = hovered_slot_in_family(cursor_position, &slot_queries.p0());
        equipment_hovered.or_else(|| hovered_slot_in_family(cursor_position, &slot_queries.p1()))
    };

    if context_menu_state.is_visible() {
        return;
    }

    if use_on_state.source.is_some() {
        return;
    }

    if mouse_input.just_pressed(MouseButton::Left) && drag_state.source.is_none() {
        if let Some(slot_kind) = hovered_slot {
            if let Some(object_id) = take_item_from_slot_kind(
                &mut inventory_state,
                &mut container_query,
                open_container_state.entity,
                slot_kind,
            ) {
                info!(
                    "drag_start ui_slot={slot_kind:?} object_id={object_id} open_container={:?}",
                    open_container_state.entity
                );
                info!(
                    "equipment_state_after_take {}",
                    equipment_state_summary(&inventory_state)
                );
                drag_state.source = Some(DragSource::UiSlot(slot_kind));
                drag_state.object_id = Some(object_id);
                drag_state.world_origin = None;
                return;
            }
        }

        let target_tile = cursor_to_tile(window, cursor_position, player_position, &world_config);

        for (entity, tile_position, object) in &movable_query {
            if *tile_position != target_tile {
                continue;
            }

            if !is_near_player(player_position, tile_position) {
                continue;
            }

            info!(
                "drag_start world_entity={entity:?} object_id={} origin=({}, {})",
                object.object_id, tile_position.x, tile_position.y
            );
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

    info!(
        "drag_release source={:?} object_id={} hovered_slot={hovered_slot:?} target_tile=({}, {})",
        drag_source.as_ref().map(drag_source_name),
        object_id,
        target_tile.x,
        target_tile.y
    );
    if hovered_slot.is_none() {
        log_equipment_slot_bounds(cursor_position, &slot_queries.p0());
    }

    match drag_source {
        Some(DragSource::World(item_entity)) => {
            if let Some(slot_kind) = hovered_slot {
                if place_item_in_slot_kind(
                    &mut inventory_state,
                    &mut container_query,
                    open_container_state.entity,
                    object_id,
                    slot_kind,
                    &object_registry,
                    &definitions,
                ) {
                    info!(
                        "equipment_state_after_world_to_slot {}",
                        equipment_state_summary(&inventory_state)
                    );
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
                    &movable_query,
                    &world_config,
                ) {
                    commands.entity(item_entity).insert(target_tile);
                }
            }
        }
        Some(DragSource::UiSlot(source_slot)) => {
            if let Some(slot_kind) = hovered_slot {
                if slot_kind == source_slot {
                    restore_item_to_slot(
                        &mut inventory_state,
                        &mut container_query,
                        open_container_state.entity,
                        source_slot,
                        object_id,
                    );
                    return;
                }

                if place_item_in_slot_kind(
                    &mut inventory_state,
                    &mut container_query,
                    open_container_state.entity,
                    object_id,
                    slot_kind,
                    &object_registry,
                    &definitions,
                ) {
                    info!(
                        "equipment_state_after_ui_to_slot {}",
                        equipment_state_summary(&inventory_state)
                    );
                    return;
                }
            }

            if let Some(world_drop_tile) = find_nearest_valid_world_drop_tile(
                target_tile,
                None,
                player_position,
                Entity::PLACEHOLDER,
                &collider_query,
                &movable_query,
                &world_config,
            ) {
                if let Some(type_id) = object_registry.type_id(object_id) {
                    spawn_overworld_object(
                        &mut commands,
                        &asset_server,
                        &definitions,
                        &world_config,
                        object_id,
                        type_id,
                        None,
                        world_drop_tile,
                    );
                    info!(
                        "equipment_state_after_ui_to_world {}",
                        equipment_state_summary(&inventory_state)
                    );
                    return;
                }
            }

            restore_item_to_slot(
                &mut inventory_state,
                &mut container_query,
                open_container_state.entity,
                source_slot,
                object_id,
            );
            info!(
                "equipment_state_after_restore {}",
                equipment_state_summary(&inventory_state)
            );
        }
        None => {}
    }
}

pub fn sync_drag_preview(
    drag_state: Res<DragState>,
    object_registry: Res<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
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

    if let Some(name) = object_registry.display_name(object_id, &definitions, &spell_definitions) {
        label.0 = name;
        return;
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

fn is_cursor_over_button<M: Component>(
    cursor_position: Vec2,
    button_query: &Query<(&ComputedNode, &UiGlobalTransform), With<M>>,
) -> bool {
    let Ok((computed_node, global_transform)) = button_query.single() else {
        return false;
    };

    point_in_ui_node(cursor_position, computed_node, global_transform)
}

fn is_cursor_over_visible_button<M: Component>(
    cursor_position: Vec2,
    button_query: &Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<M>>,
) -> bool {
    let Ok((computed_node, global_transform, visibility)) = button_query.single() else {
        return false;
    };
    if *visibility == Visibility::Hidden {
        return false;
    }

    point_in_ui_node(cursor_position, computed_node, global_transform)
}

fn hovered_slot_kind_from_ui(
    cursor_position: Vec2,
    slot_queries: &mut ParamSet<(
        Query<
            (
                &ItemSlotButton,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            (With<Button>, With<EquipmentSlotButton>),
        >,
        Query<
            (
                &ItemSlotButton,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            (With<Button>, With<ContainerSlotButton>),
        >,
        Query<
            (
                &ItemSlotImage,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            With<EquipmentSlotImage>,
        >,
        Query<
            (
                &ItemSlotImage,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            With<ContainerSlotImage>,
        >,
    )>,
) -> Option<ItemSlotKind> {
    if let Some(kind) = hovered_slot_in_family(cursor_position, &slot_queries.p0()) {
        return Some(kind);
    }
    if let Some(kind) = hovered_slot_in_family(cursor_position, &slot_queries.p1()) {
        return Some(kind);
    }
    if let Some(kind) = hovered_slot_image_in_family(cursor_position, &slot_queries.p2()) {
        return Some(kind);
    }
    hovered_slot_image_in_family(cursor_position, &slot_queries.p3())
}

fn hovered_slot_image_in_family<F: QueryFilter>(
    cursor_position: Vec2,
    slot_query: &Query<
        (
            &ItemSlotImage,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&Visibility>,
        ),
        F,
    >,
) -> Option<ItemSlotKind> {
    slot_query
        .iter()
        .find_map(|(slot, computed_node, global_transform, visibility)| {
            if visibility.is_some_and(|visibility| *visibility == Visibility::Hidden) {
                return None;
            }

            point_in_ui_node(cursor_position, computed_node, global_transform).then_some(slot.kind)
        })
}

fn hovered_slot_in_family<F: QueryFilter>(
    cursor_position: Vec2,
    slot_query: &Query<
        (
            &ItemSlotButton,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&Visibility>,
        ),
        F,
    >,
) -> Option<ItemSlotKind> {
    slot_query
        .iter()
        .find_map(|(slot, computed_node, global_transform, visibility)| {
            if visibility.is_some_and(|visibility| *visibility == Visibility::Hidden) {
                return None;
            }

            point_in_ui_node(cursor_position, computed_node, global_transform).then_some(slot.kind)
        })
}

fn point_in_ui_node(
    cursor_position: Vec2,
    computed_node: &ComputedNode,
    global_transform: &UiGlobalTransform,
) -> bool {
    computed_node.contains_point(*global_transform, cursor_position)
}

fn take_item_from_slot_kind(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    open_container_entity: Option<Entity>,
    slot_kind: ItemSlotKind,
) -> Option<u64> {
    match slot_kind {
        ItemSlotKind::ActiveContainer(slot_index) => {
            if let Some(entity) = open_container_entity {
                return container_query
                    .get_mut(entity)
                    .ok()
                    .and_then(|mut container| container.slots.get_mut(slot_index)?.take());
            }

            inventory_state.backpack_slots.get_mut(slot_index)?.take()
        }
        ItemSlotKind::Equipment(slot) => inventory_state.take_equipment_item(slot),
    }
}

fn object_id_in_slot_kind(
    inventory_state: &InventoryState,
    container_query: &Query<&Container>,
    open_container_entity: Option<Entity>,
    slot_kind: ItemSlotKind,
) -> Option<u64> {
    match slot_kind {
        ItemSlotKind::ActiveContainer(slot_index) => {
            if let Some(entity) = open_container_entity {
                return container_query
                    .get(entity)
                    .ok()
                    .and_then(|container| container.slots.get(slot_index).copied().flatten());
            }

            inventory_state
                .backpack_slots
                .get(slot_index)
                .copied()
                .flatten()
        }
        ItemSlotKind::Equipment(slot) => inventory_state.equipment_item(slot),
    }
}

fn active_object_id_in_container_view(
    inventory_state: &InventoryState,
    container_query: &Query<&Container>,
    open_container_entity: Option<Entity>,
    slot_index: usize,
) -> Option<u64> {
    if let Some(entity) = open_container_entity {
        return container_query
            .get(entity)
            .ok()
            .and_then(|container| container.slots.get(slot_index).copied().flatten());
    }

    inventory_state
        .backpack_slots
        .get(slot_index)
        .copied()
        .flatten()
}

fn place_item_in_slot_kind(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    open_container_entity: Option<Entity>,
    object_id: u64,
    slot_kind: ItemSlotKind,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    if !object_is_storable(object_id, object_registry, definitions) {
        return false;
    }

    match slot_kind {
        ItemSlotKind::ActiveContainer(slot_index) => {
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
                true
            } else {
                let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) else {
                    return false;
                };
                if slot.is_some() {
                    return false;
                }
                *slot = Some(object_id);
                true
            }
        }
        ItemSlotKind::Equipment(slot) => place_item_in_equipment_slot(
            inventory_state,
            object_registry,
            definitions,
            slot,
            object_id,
        ),
    }
}

fn restore_backpack_slot(inventory_state: &mut InventoryState, slot_index: usize, object_id: u64) {
    if let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) {
        *slot = Some(object_id);
    }
}

fn restore_item_to_slot(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    open_container_entity: Option<Entity>,
    slot_kind: ItemSlotKind,
    object_id: u64,
) {
    match slot_kind {
        ItemSlotKind::ActiveContainer(slot_index) => {
            if let Some(entity) = open_container_entity {
                restore_container_slot(container_query, entity, slot_index, object_id);
            } else {
                restore_backpack_slot(inventory_state, slot_index, object_id);
            }
        }
        ItemSlotKind::Equipment(slot) => {
            inventory_state.restore_equipment_item(slot, object_id);
        }
    }
}

fn place_item_in_equipment_slot(
    inventory_state: &mut InventoryState,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    slot: EquipmentSlot,
    object_id: u64,
) -> bool {
    let Some(type_id) = object_registry.type_id(object_id) else {
        warn!(
            "equip_reject object_id={} reason=missing_type_id",
            object_id
        );
        return false;
    };
    let Some(definition) = definitions.get(type_id) else {
        warn!(
            "equip_reject object_id={} type_id={} reason=missing_definition",
            object_id, type_id
        );
        return false;
    };
    if definition.equipment_slot != Some(slot) {
        warn!(
            "equip_reject object_id={} type_id={} target_slot={slot:?} item_slot={:?} reason=slot_mismatch",
            object_id,
            type_id,
            definition.equipment_slot
        );
        return false;
    }

    let placed = inventory_state.place_equipment_item(slot, object_id);
    if placed {
        info!(
            "equip_accept object_id={} type_id={} slot={slot:?}",
            object_id, type_id
        );
    } else {
        warn!(
            "equip_reject object_id={} type_id={} slot={slot:?} reason=slot_occupied",
            object_id, type_id
        );
    }

    placed
}

fn drag_source_name(source: &DragSource) -> &'static str {
    match source {
        DragSource::World(_) => "world",
        DragSource::UiSlot(_) => "ui_slot",
    }
}

fn equipment_state_summary(inventory_state: &InventoryState) -> String {
    inventory_state
        .equipment_slots
        .iter()
        .map(|(slot, object_id)| format!("{slot:?}={object_id:?}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn log_equipment_slot_bounds(
    cursor_position: Vec2,
    equipment_query: &Query<
        (
            &ItemSlotButton,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&Visibility>,
        ),
        impl QueryFilter,
    >,
) {
    for (slot, computed_node, global_transform, visibility) in equipment_query {
        let center = global_transform.transform_point2(Vec2::ZERO);
        let visible = visibility.is_none_or(|visibility| *visibility != Visibility::Hidden);
        let contains_cursor = point_in_ui_node(cursor_position, computed_node, global_transform);

        info!(
            "equipment_slot_debug kind={:?} center=({:.1}, {:.1}) size=({:.1}, {:.1}) visible={} contains_cursor={}",
            slot.kind,
            center.x,
            center.y,
            computed_node.size().x,
            computed_node.size().y,
            visible,
            contains_cursor
        );
    }
}

fn object_description(
    object_id: u64,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
) -> Option<String> {
    let type_id = object_registry.type_id(object_id)?;
    let definition = definitions.get(type_id)?;
    let mut parts = Vec::new();
    let display_name = object_registry
        .display_name(object_id, definitions, spell_definitions)
        .unwrap_or_else(|| definition.name.clone());
    let description_text = object_registry
        .description(object_id, definitions, spell_definitions)
        .unwrap_or_else(|| definition.description.clone());
    let description = description_text.trim();
    if description.is_empty() {
        parts.push(format!("Just a {}.", display_name.to_lowercase()));
    } else {
        parts.push(description.to_owned());
    }

    let stat_lines = stat_bonus_lines(definition);
    if !stat_lines.is_empty() {
        parts.push(format!("Bonuses: {}", stat_lines.join(", ")));
    }

    Some(parts.join(" "))
}

fn inspect_context_target(
    target: ContextMenuTarget,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    chat_log_state: &mut ChatLogState,
) {
    let object_id = match target {
        ContextMenuTarget::World(_, object_id) | ContextMenuTarget::Slot(_, object_id) => object_id,
    };

    if let Some(description) =
        object_description(object_id, object_registry, definitions, spell_definitions)
    {
        chat_log_state.push_narrator(description);
    }
}

fn handle_use_action(
    source_target: ContextMenuTarget,
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    open_container_entity: Option<Entity>,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    player_query: &mut Query<&mut VitalStats, With<Player>>,
    chat_log_state: &mut ChatLogState,
    cursor_state: &mut CursorState,
    spell_targeting_state: &mut SpellTargetingState,
    commands: &mut Commands,
) {
    let object_id = context_target_object_id(source_target);
    let Some(type_id) = object_registry.type_id(object_id) else {
        return;
    };
    let Some(definition) = definitions.get(type_id) else {
        return;
    };
    let source_name = object_registry
        .display_name(object_id, definitions, spell_definitions)
        .unwrap_or_else(|| definition.name.clone());

    let Some(spell_id) =
        object_registry.resolved_spell_id(object_id, definitions, spell_definitions)
    else {
        use_on_player_target(
            source_target,
            inventory_state,
            container_query,
            open_container_entity,
            object_registry,
            definitions,
            spell_definitions,
            player_query,
            chat_log_state,
            commands,
        );
        return;
    };
    let Some(spell) = spell_definitions.get(&spell_id) else {
        chat_log_state.push_narrator(format!(
            "{} is inscribed with an unknown spell.",
            source_name
        ));
        return;
    };

    match spell.targeting {
        SpellTargeting::Untargeted => {
            cast_untargeted_spell(
                source_target,
                spell,
                open_container_entity,
                inventory_state,
                container_query,
                player_query,
                chat_log_state,
                commands,
            );
        }
        SpellTargeting::Targeted => {
            spell_targeting_state.source = Some(source_target);
            spell_targeting_state.spell_id = Some(spell_id);
            cursor_state.mode = CursorMode::SpellTarget;
            chat_log_state.push_narrator(format!("Select a target for {}.", spell.name));
        }
    }
}

fn use_on_player_target(
    source_target: ContextMenuTarget,
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    open_container_entity: Option<Entity>,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    player_query: &mut Query<&mut VitalStats, With<Player>>,
    chat_log_state: &mut ChatLogState,
    commands: &mut Commands,
) {
    let object_id = context_target_object_id(source_target);

    let Some(type_id) = object_registry.type_id(object_id) else {
        return;
    };
    let Some(definition) = definitions.get(type_id) else {
        return;
    };
    let source_name = object_registry
        .display_name(object_id, definitions, spell_definitions)
        .unwrap_or_else(|| definition.name.clone());
    if !definition.is_usable() {
        return;
    }

    let Ok(mut vital_stats) = player_query.single_mut() else {
        return;
    };

    vital_stats.health = (vital_stats.health + definition.use_effects.restore_health)
        .clamp(0.0, vital_stats.max_health);
    vital_stats.mana =
        (vital_stats.mana + definition.use_effects.restore_mana).clamp(0.0, vital_stats.max_mana);

    match source_target {
        ContextMenuTarget::World(entity, _) => {
            commands.entity(entity).despawn();
        }
        ContextMenuTarget::Slot(slot_kind, _) => {
            let _ = take_item_from_slot_kind(
                inventory_state,
                container_query,
                open_container_entity,
                slot_kind,
            );
        }
    }

    chat_log_state.push_narrator(use_text(definition, &source_name));
}

fn cast_untargeted_spell(
    source_target: ContextMenuTarget,
    spell: &SpellDefinition,
    open_container_entity: Option<Entity>,
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    player_query: &mut Query<&mut VitalStats, With<Player>>,
    chat_log_state: &mut ChatLogState,
    commands: &mut Commands,
) {
    let Ok(mut player_vitals) = player_query.single_mut() else {
        return;
    };

    if player_vitals.mana < spell.mana_cost {
        chat_log_state.push_narrator(format!("Not enough mana to cast {}.", spell.name));
        return;
    }

    player_vitals.mana = (player_vitals.mana - spell.mana_cost).max(0.0);
    apply_spell_effects(spell, &mut player_vitals);

    consume_source_target(
        source_target,
        inventory_state,
        container_query,
        open_container_entity,
        commands,
    );

    chat_log_state.push_line(format!("[Player]: \"{}\"", spell.incantation));
    chat_log_state.push_narrator(format!("Cast {}.", spell.name));
}

fn consume_source_target(
    source_target: ContextMenuTarget,
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    open_container_entity: Option<Entity>,
    commands: &mut Commands,
) {
    match source_target {
        ContextMenuTarget::World(entity, _) => {
            commands.entity(entity).despawn();
        }
        ContextMenuTarget::Slot(slot_kind, _) => {
            let _ = take_item_from_slot_kind(
                inventory_state,
                container_query,
                open_container_entity,
                slot_kind,
            );
        }
    }
}

fn apply_spell_effects(spell: &SpellDefinition, vital_stats: &mut VitalStats) {
    vital_stats.health =
        (vital_stats.health - spell.effects.damage).clamp(0.0, vital_stats.max_health);
    vital_stats.health =
        (vital_stats.health + spell.effects.restore_health).clamp(0.0, vital_stats.max_health);
    vital_stats.mana =
        (vital_stats.mana + spell.effects.restore_mana).clamp(0.0, vital_stats.max_mana);
}

fn chebyshev_distance_tiles(a: TilePosition, b: TilePosition) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

fn use_on_world_target(
    source_target: ContextMenuTarget,
    target_object_id: u64,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    chat_log_state: &mut ChatLogState,
) {
    let source_object_id = context_target_object_id(source_target);
    let Some(source_type_id) = object_registry.type_id(source_object_id) else {
        return;
    };
    let Some(source_definition) = definitions.get(source_type_id) else {
        return;
    };
    let Some(target_type_id) = object_registry.type_id(target_object_id) else {
        return;
    };
    let Some(target_definition) = definitions.get(target_type_id) else {
        return;
    };
    let source_name = object_registry
        .display_name(source_object_id, definitions, spell_definitions)
        .unwrap_or_else(|| source_definition.name.clone());
    let target_name = object_registry
        .display_name(target_object_id, definitions, spell_definitions)
        .unwrap_or_else(|| target_definition.name.clone());

    chat_log_state.push_narrator(use_on_text(source_definition, &source_name, &target_name));
}

fn object_name(
    object_id: u64,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
) -> Option<String> {
    object_registry.display_name(object_id, definitions, spell_definitions)
}

fn object_is_usable(
    object_id: u64,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    let Some(type_id) = object_registry.type_id(object_id) else {
        return false;
    };
    let Some(definition) = definitions.get(type_id) else {
        return false;
    };

    definition.is_usable()
}

fn object_is_storable(
    object_id: u64,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    let Some(type_id) = object_registry.type_id(object_id) else {
        return false;
    };
    let Some(definition) = definitions.get(type_id) else {
        return false;
    };

    definition.storable
}

fn can_use_on(
    object_id: u64,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
) -> bool {
    let Some(type_id) = object_registry.type_id(object_id) else {
        return false;
    };
    let Some(definition) = definitions.get(type_id) else {
        return false;
    };

    definition.is_usable()
        && object_registry
            .resolved_spell_id(object_id, definitions, spell_definitions)
            .is_none()
}

fn context_target_object_id(target: ContextMenuTarget) -> u64 {
    match target {
        ContextMenuTarget::World(_, object_id) | ContextMenuTarget::Slot(_, object_id) => object_id,
    }
}

fn use_text(definition: &OverworldObjectDefinition, item_name: &str) -> String {
    if definition.use_texts.is_empty() {
        return format!("{item_name} used.");
    }

    random_text(&definition.use_texts).replace("{item}", item_name)
}

fn use_on_text(
    definition: &OverworldObjectDefinition,
    item_name: &str,
    target_name: &str,
) -> String {
    if definition.use_on_texts.is_empty() {
        return format!("Used {} on {}.", item_name, target_name);
    }

    let template = random_text(&definition.use_on_texts);
    template
        .replace("{target}", target_name)
        .replace("{item}", item_name)
}

fn random_text(texts: &[String]) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    texts[nanos % texts.len()].clone()
}

fn stat_bonus_lines(definition: &OverworldObjectDefinition) -> Vec<String> {
    let mut bonuses = Vec::new();

    if definition.stats.strength != 0 {
        bonuses.push(format!("{:+} str", definition.stats.strength));
    }
    if definition.stats.agility != 0 {
        bonuses.push(format!("{:+} agi", definition.stats.agility));
    }
    if definition.stats.constitution != 0 {
        bonuses.push(format!("{:+} con", definition.stats.constitution));
    }
    if definition.stats.willpower != 0 {
        bonuses.push(format!("{:+} wil", definition.stats.willpower));
    }
    if definition.stats.charisma != 0 {
        bonuses.push(format!("{:+} cha", definition.stats.charisma));
    }
    if definition.stats.focus != 0 {
        bonuses.push(format!("{:+} foc", definition.stats.focus));
    }
    if definition.stats.max_health != 0 {
        bonuses.push(format!("{:+} hp", definition.stats.max_health));
    }
    if definition.stats.max_mana != 0 {
        bonuses.push(format!("{:+} mana", definition.stats.max_mana));
    }
    if definition.stats.storage_slots != 0 {
        bonuses.push(format!("{:+} storage", definition.stats.storage_slots));
    }

    bonuses
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
    movable_query: &Query<(Entity, &TilePosition, &OverworldObject), With<Movable>>,
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

    !movable_query
        .iter()
        .any(|(entity, tile, _)| entity != dragged_entity && *tile == target_tile)
}

fn find_nearest_valid_world_drop_tile(
    requested_tile: TilePosition,
    source_world_tile: Option<TilePosition>,
    player_position: &TilePosition,
    dragged_entity: Entity,
    collider_query: &Query<&TilePosition, (With<Collider>, Without<Player>)>,
    movable_query: &Query<(Entity, &TilePosition, &OverworldObject), With<Movable>>,
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
            movable_query,
            world_config,
        ) {
            return Some(candidate);
        }
    }

    None
}
