use bevy::ecs::query::QueryFilter;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::log::info;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow};

use crate::game::commands::{
    GameCommand, InspectTarget, ItemDestination, ItemReference, ItemSlotRef, UseTarget,
};
use crate::game::resources::{
    ClientGameState, GameUiEvent, InventoryState, PendingGameCommands, PendingGameUiEvents,
};
use crate::magic::resources::{SpellDefinitions, SpellTargeting};
use crate::player::components::InventoryStack;
use crate::scripting::resources::PythonConsoleState;
use crate::ui::components::{
    BackpackSlotRow, ChatLogText, ContainerSlotButton, ContainerSlotImage, ContextMenuAttackButton,
    ContextMenuInspectButton, ContextMenuOpenButton, ContextMenuRoot, ContextMenuTakePartialButton,
    ContextMenuUseButton, ContextMenuUseOnButton, CurrentCombatTargetLabel, DockedPanelBody,
    DockedPanelCanvas, DockedPanelCloseButton, DockedPanelDragHandle, DockedPanelResizeHandle,
    DockedPanelRoot, DockedPanelTitle, DragPreviewLabel, DragPreviewRoot, EquipmentSlotButton,
    EquipmentSlotImage, HealthFill, ItemSlotButton, ItemSlotImage, ItemSlotKind,
    ItemSlotQuantityLabel, ManaFill, RightSidebarRoot, TakePartialAmountLabel,
    TakePartialCancelButton, TakePartialConfirmButton, TakePartialDecButton, TakePartialIncButton,
    TakePartialPopupRoot,
};
use crate::ui::resources::{
    ContextMenuState, ContextMenuTarget, CursorMode, CursorState, DockedPanelDragState,
    DockedPanelKind, DockedPanelResizeState, DockedPanelState, DragSource, DragState,
    SpellTargetingState, TakePartialState, UseOnState,
};
use crate::world::components::TilePosition;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::WorldConfig;

pub fn apply_game_ui_events(
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    mut docked_panel_state: ResMut<DockedPanelState>,
    mut dialog_state: ResMut<crate::ui::resources::ActiveDialogState>,
) {
    let events = std::mem::take(&mut pending_ui_events.events);

    for event in events {
        match event {
            GameUiEvent::OpenContainer { object_id } => {
                docked_panel_state.open(object_id);
            }
            GameUiEvent::DialogLine {
                session_id,
                speaker,
                text,
            } => {
                dialog_state.show_line(session_id, speaker, text);
            }
            GameUiEvent::DialogOptions {
                session_id,
                options,
            } => {
                dialog_state.show_options(session_id, options);
            }
            GameUiEvent::DialogClose { .. } => {
                dialog_state.close();
            }
            other @ GameUiEvent::ProjectileFired { .. } => {
                pending_ui_events.events.push(other);
            }
        }
    }
}

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
            CursorMode::AttackTarget => CursorMode::AttackTarget,
        };
    }

    let ctrl_held = keyboard_input.pressed(KeyCode::ControlLeft)
        || keyboard_input.pressed(KeyCode::ControlRight);
    if ctrl_held && keyboard_input.just_pressed(KeyCode::KeyA) {
        cursor_state.mode = match cursor_state.mode {
            CursorMode::AttackTarget => CursorMode::Default,
            _ => CursorMode::AttackTarget,
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
        CursorMode::AttackTarget => "cursors/attack_cursor.png",
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
    client_state: Res<ClientGameState>,
    mut docked_panel_state: ResMut<DockedPanelState>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    let player_position = client_state.player_tile_position;

    let before_ids: Vec<u64> = docked_panel_state
        .panels
        .iter()
        .filter_map(|panel| match panel.kind {
            DockedPanelKind::Container { object_id } => Some(object_id),
            _ => None,
        })
        .collect();

    docked_panel_state.panels.retain(|panel| match panel.kind {
        DockedPanelKind::Minimap
        | DockedPanelKind::Status
        | DockedPanelKind::Equipment
        | DockedPanelKind::Backpack
        | DockedPanelKind::CurrentTarget => true,
        DockedPanelKind::Container { object_id } => player_position
            .and_then(|player_position| {
                client_state
                    .world_objects
                    .get(&object_id)
                    .map(|object| (player_position, object))
            })
            .is_some_and(|(player_position, object)| {
                object.is_container && is_near_player(&player_position, &object.tile_position)
            }),
    });

    let after_ids: std::collections::HashSet<u64> = docked_panel_state
        .panels
        .iter()
        .filter_map(|panel| match panel.kind {
            DockedPanelKind::Container { object_id } => Some(object_id),
            _ => None,
        })
        .collect();

    for evicted in before_ids.into_iter().filter(|id| !after_ids.contains(id)) {
        pending_commands.push(GameCommand::CloseContainer { object_id: evicted });
    }
}

pub fn handle_docked_panel_close_buttons(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut docked_panel_state: ResMut<DockedPanelState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    button_query: Query<
        (
            &DockedPanelCloseButton,
            &ComputedNode,
            &UiGlobalTransform,
            &Visibility,
        ),
        With<DockedPanelCloseButton>,
    >,
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

    for (button, computed_node, global_transform, visibility) in &button_query {
        if *visibility == Visibility::Hidden {
            continue;
        }
        if point_in_ui_node(cursor_position, computed_node, global_transform) {
            match docked_panel_state
                .panel(button.panel_id)
                .map(|panel| panel.kind)
            {
                Some(DockedPanelKind::CurrentTarget) => {
                    pending_commands.push(GameCommand::SetCombatTarget {
                        target_object_id: None,
                    });
                    docked_panel_state.close_current_target();
                }
                Some(DockedPanelKind::Container { object_id }) => {
                    pending_commands.push(GameCommand::CloseContainer { object_id });
                    docked_panel_state.close_panel(button.panel_id);
                }
                Some(DockedPanelKind::Minimap)
                | Some(DockedPanelKind::Status)
                | Some(DockedPanelKind::Equipment)
                | Some(DockedPanelKind::Backpack) => {}
                None => {}
            }
            return;
        }
    }
}

pub fn sync_current_combat_target(
    client_state: Res<ClientGameState>,
    object_registry: Res<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    mut docked_panel_state: ResMut<DockedPanelState>,
    mut label_query: Query<&mut Text, With<CurrentCombatTargetLabel>>,
) {
    let Ok(mut label) = label_query.single_mut() else {
        return;
    };

    let text = if let Some(target_object_id) = client_state.current_target_object_id {
        docked_panel_state.open_current_target();
        let name = object_registry
            .display_name(target_object_id, &definitions, &spell_definitions)
            .unwrap_or_else(|| target_object_id.to_string());
        format!("Target: {name}")
    } else {
        docked_panel_state.close_current_target();
        "Target: none".to_owned()
    };

    label.0 = text;
}

pub fn sync_vital_bars(
    client_state: Res<ClientGameState>,
    mut health_query: Query<&mut Node, With<HealthFill>>,
    mut mana_query: Query<&mut Node, (With<ManaFill>, Without<HealthFill>)>,
) {
    let Some(vital_stats) = client_state.player_vitals else {
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

pub fn sync_chat_log(
    client_state: Res<ClientGameState>,
    mut chat_query: Query<&mut Text, With<ChatLogText>>,
) {
    let Ok(mut chat_text) = chat_query.single_mut() else {
        return;
    };

    chat_text.0 = client_state.chat_log_lines.join("\n");
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
    mut open_button_query: Query<&mut Node, With<ContextMenuOpenButton>>,
) {
    let Ok(mut node) = open_button_query.single_mut() else {
        return;
    };

    node.display = if context_menu_state.can_open {
        Display::Flex
    } else {
        Display::None
    };
}

pub fn sync_context_menu_interact_button(
    context_menu_state: Res<ContextMenuState>,
    mut button_query: Query<
        (&mut Node, &Children),
        With<crate::ui::components::ContextMenuInteractButton>,
    >,
    mut text_query: Query<&mut Text>,
) {
    let Ok((mut node, children)) = button_query.single_mut() else {
        return;
    };

    if let Some((_, label)) = &context_menu_state.interaction {
        node.display = Display::Flex;
        for child in children.iter() {
            if let Ok(mut text) = text_query.get_mut(child) {
                **text = label.clone();
            }
        }
    } else {
        node.display = Display::None;
    }
}

pub fn sync_context_menu_attack_button(
    context_menu_state: Res<ContextMenuState>,
    mut attack_button_query: Query<&mut Node, With<ContextMenuAttackButton>>,
) {
    let Ok(mut node) = attack_button_query.single_mut() else {
        return;
    };

    node.display = if context_menu_state.can_attack {
        Display::Flex
    } else {
        Display::None
    };
}

pub fn sync_context_menu_talk_button(
    context_menu_state: Res<ContextMenuState>,
    mut button_query: Query<&mut Node, With<crate::ui::components::ContextMenuTalkButton>>,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };

    node.display = if context_menu_state.can_talk {
        Display::Flex
    } else {
        Display::None
    };
}

pub fn sync_context_menu_use_button(
    context_menu_state: Res<ContextMenuState>,
    mut use_button_query: Query<&mut Node, With<ContextMenuUseButton>>,
) {
    let Ok(mut node) = use_button_query.single_mut() else {
        return;
    };

    node.display = if context_menu_state.can_use {
        Display::Flex
    } else {
        Display::None
    };
}

pub fn sync_context_menu_use_on_button(
    context_menu_state: Res<ContextMenuState>,
    mut use_on_button_query: Query<&mut Node, With<ContextMenuUseOnButton>>,
) {
    let Ok(mut node) = use_on_button_query.single_mut() else {
        return;
    };

    node.display = if context_menu_state.can_use_on {
        Display::Flex
    } else {
        Display::None
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
    mut pending_commands: ResMut<PendingGameCommands>,
    client_state: Res<ClientGameState>,
    mut take_partial_state: ResMut<TakePartialState>,
    ui_state: (
        ResMut<ContextMenuState>,
        Res<DockedPanelState>,
        ResMut<CursorState>,
        ResMut<UseOnState>,
        ResMut<SpellTargetingState>,
    ),
    mut menu_queries: ParamSet<(
        Query<(&ComputedNode, &UiGlobalTransform), With<ContextMenuAttackButton>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<ContextMenuInspectButton>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<ContextMenuOpenButton>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<ContextMenuUseButton>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<ContextMenuUseOnButton>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<ContextMenuTakePartialButton>>,
        Query<
            (&ComputedNode, &UiGlobalTransform),
            With<crate::ui::components::ContextMenuTalkButton>,
        >,
        Query<
            (&ComputedNode, &UiGlobalTransform),
            With<crate::ui::components::ContextMenuInteractButton>,
        >,
    )>,
) {
    let (object_registry, definitions, spell_definitions) = static_resources;
    let (
        mut context_menu_state,
        docked_panel_state,
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

    if is_cursor_over_button(cursor_position, &menu_queries.p6()) {
        if let Some(ContextMenuTarget::World(object_id)) = context_menu_state.target {
            pending_commands.push(GameCommand::TalkToNpc {
                npc_object_id: object_id,
            });
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_button(cursor_position, &menu_queries.p0()) {
        if let Some(ContextMenuTarget::World(object_id)) = context_menu_state.target {
            pending_commands.push(GameCommand::SetCombatTarget {
                target_object_id: Some(object_id),
            });
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_button(cursor_position, &menu_queries.p1()) {
        if let Some(target) = context_menu_state.target {
            let inspect_target = match target {
                ContextMenuTarget::Slot(kind) => item_slot_kind_to_ref(kind, &docked_panel_state)
                    .map(InspectTarget::SlotItem),
                ContextMenuTarget::World(object_id) => Some(InspectTarget::Object(object_id)),
            };
            if let Some(inspect_target) = inspect_target {
                pending_commands.push(GameCommand::Inspect {
                    target: inspect_target,
                });
            }
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_button(cursor_position, &menu_queries.p3()) {
        if let Some(target) = context_menu_state.target {
            let spell_lookup = match target {
                ContextMenuTarget::World(object_id) => {
                    object_registry.resolved_spell_id(object_id, &definitions, &spell_definitions)
                }
                ContextMenuTarget::Slot(slot_kind) => {
                    stack_in_slot_kind(&client_state, &docked_panel_state, slot_kind).and_then(
                        |stack| {
                            ObjectRegistry::resolved_spell_id_for_type(
                                &stack.type_id,
                                Some(&stack.properties),
                                &definitions,
                                &spell_definitions,
                            )
                        },
                    )
                }
            };
            if let Some(spell_id) = spell_lookup {
                if let Some(spell) = spell_definitions.get(&spell_id) {
                    if spell.targeting == SpellTargeting::Targeted {
                        spell_targeting_state.source = Some(target);
                        spell_targeting_state.spell_id = Some(spell_id);
                        cursor_state.mode = CursorMode::SpellTarget;
                        context_menu_state.hide();
                        return;
                    }
                }
            }

            if let Some(source) = context_target_to_item_reference(target, &docked_panel_state) {
                pending_commands.push(GameCommand::UseItem { source });
            }
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_button(cursor_position, &menu_queries.p4()) {
        if let Some(target) = context_menu_state.target {
            let usable = match target {
                ContextMenuTarget::World(object_id) => {
                    object_is_usable(object_id, &object_registry, &definitions)
                }
                ContextMenuTarget::Slot(slot_kind) => {
                    stack_in_slot_kind(&client_state, &docked_panel_state, slot_kind)
                        .map(|stack| {
                            definitions
                                .get(&stack.type_id)
                                .is_some_and(|d| d.is_usable())
                        })
                        .unwrap_or(false)
                }
            };
            if usable {
                use_on_state.source = Some(target);
                cursor_state.mode = CursorMode::UseOn;
            }
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_button(cursor_position, &menu_queries.p2()) {
        if let Some(ContextMenuTarget::World(object_id)) = context_menu_state.target {
            pending_commands.push(GameCommand::OpenContainer { object_id });
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_button(cursor_position, &menu_queries.p7()) {
        if let (Some(ContextMenuTarget::World(object_id)), Some((verb, _label))) = (
            context_menu_state.target,
            context_menu_state.interaction.clone(),
        ) {
            pending_commands.push(GameCommand::InteractWithObject { object_id, verb });
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_button(cursor_position, &menu_queries.p5()) {
        match context_menu_state.target {
            Some(ContextMenuTarget::Slot(slot_kind)) => {
                if let Some(slot_ref) = item_slot_kind_to_ref(slot_kind, &docked_panel_state) {
                    if let Some(stack) =
                        stack_in_slot_kind(&client_state, &docked_panel_state, slot_kind)
                    {
                        take_partial_state.source = Some(ItemReference::Slot(slot_ref));
                        take_partial_state.max_amount = stack.quantity;
                        take_partial_state.selected_amount = 1;
                    }
                }
            }
            Some(ContextMenuTarget::World(object_id)) => {
                if let Some(obj) = client_state.world_objects.get(&object_id) {
                    take_partial_state.source = Some(ItemReference::WorldObject(object_id));
                    take_partial_state.max_amount = obj.quantity;
                    take_partial_state.selected_amount = 1;
                }
            }
            None => {}
        }
        context_menu_state.hide();
        return;
    }

    context_menu_state.hide();
}

pub fn handle_use_on_targeting(
    mouse_input: Res<ButtonInput<MouseButton>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    state_resources: (Res<ContextMenuState>, Res<DockedPanelState>),
    client_state: Res<ClientGameState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut cursor_state: ResMut<CursorState>,
    mut use_on_state: ResMut<UseOnState>,
) {
    let (context_menu_state, docked_panel_state) = state_resources;
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
    let Some(player_position) = client_state.player_tile_position else {
        return;
    };

    let target_tile = cursor_to_tile(window, cursor_position, &player_position, &world_config);
    let Some(source) = context_target_to_item_reference(source_target, &docked_panel_state) else {
        return;
    };

    if target_tile == player_position {
        pending_commands.push(GameCommand::UseItemOn {
            source,
            target: UseTarget::Player,
        });
        use_on_state.source = None;
        cursor_state.mode = CursorMode::Default;
        return;
    }

    for object in client_state.world_objects.values() {
        if object.tile_position != target_tile {
            continue;
        }
        if !is_near_player(&player_position, &object.tile_position) {
            continue;
        }

        pending_commands.push(GameCommand::UseItemOn {
            source,
            target: UseTarget::Object(object.object_id),
        });
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
    static_resources: (Res<ContextMenuState>, Res<DockedPanelState>),
    client_state: Res<ClientGameState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut cursor_state: ResMut<CursorState>,
    mut spell_targeting_state: ResMut<SpellTargetingState>,
) {
    let (context_menu_state, docked_panel_state) = static_resources;
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

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    let Some(player_position) = client_state.player_tile_position else {
        return;
    };
    let target_tile = cursor_to_tile(window, cursor_position, &player_position, &world_config);

    let selected_target = client_state
        .world_objects
        .values()
        .find(|object| object.is_npc && object.tile_position == target_tile)
        .map(|object| object.object_id);

    let Some(target_object_id) = selected_target else {
        return;
    };

    if let Some(source) = context_target_to_item_reference(source_target, &docked_panel_state) {
        pending_commands.push(GameCommand::CastSpellAt {
            source,
            spell_id: spell_id.to_owned(),
            target_object_id,
        });
    }

    spell_targeting_state.source = None;
    spell_targeting_state.spell_id = None;
    cursor_state.mode = CursorMode::Default;
}

pub fn handle_attack_targeting(
    mouse_input: Res<ButtonInput<MouseButton>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    context_menu_state: Res<ContextMenuState>,
    client_state: Res<ClientGameState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut cursor_state: ResMut<CursorState>,
) {
    if cursor_state.mode != CursorMode::AttackTarget {
        return;
    }

    if keyboard_input.just_pressed(KeyCode::Escape) || mouse_input.just_pressed(MouseButton::Right)
    {
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
    let Some(player_position) = client_state.player_tile_position else {
        return;
    };

    let target_tile = cursor_to_tile(window, cursor_position, &player_position, &world_config);

    let world_target = client_state
        .world_objects
        .values()
        .find(|object| object.is_npc && object.tile_position == target_tile)
        .map(|object| object.object_id);
    let remote_target = client_state
        .remote_players
        .values()
        .find(|player| player.tile_position == target_tile)
        .map(|player| player.object_id);

    let Some(target_object_id) = world_target.or(remote_target) else {
        return;
    };

    pending_commands.push(GameCommand::SetCombatTarget {
        target_object_id: Some(target_object_id),
    });

    cursor_state.mode = CursorMode::Default;
}

pub fn handle_context_menu_opening(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    object_registry: Res<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    client_state: Res<ClientGameState>,
    mut context_menu_state: ResMut<ContextMenuState>,
    docked_panel_state: Res<DockedPanelState>,
    mut use_on_state: ResMut<UseOnState>,
    mut spell_targeting_state: ResMut<SpellTargetingState>,
    mut cursor_state: ResMut<CursorState>,
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
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Some(player_position) = client_state.player_tile_position else {
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
        "context_open_attempt cursor=({:.1}, {:.1}) open_containers={} hovered_slot={hovered_slot:?}",
        cursor_position.x,
        cursor_position.y,
        docked_panel_state
            .panels
            .iter()
            .filter(|panel| matches!(panel.kind, DockedPanelKind::Container { .. }))
            .count()
    );
    if let Some(slot_kind) = hovered_slot {
        if let Some(stack) = stack_in_slot_kind(&client_state, &docked_panel_state, slot_kind) {
            let definition = definitions.get(&stack.type_id);
            let can_use = definition.is_some_and(|d| d.is_usable());
            let has_use_on = ObjectRegistry::resolved_spell_id_for_type(
                &stack.type_id,
                Some(&stack.properties),
                &definitions,
                &spell_definitions,
            )
            .is_some()
                || can_use;
            let stack_qty = stack.quantity;
            context_menu_state.show(
                cursor_position,
                ContextMenuTarget::Slot(slot_kind),
                false,
                can_use,
                has_use_on,
                false,
                stack_qty > 1,
                false,
                None,
            );
            info!(
                "context_open_slot_success slot={slot_kind:?} type_id={} can_use={can_use}",
                stack.type_id
            );
            return;
        }
    }

    let target_tile = cursor_to_tile(window, cursor_position, &player_position, &world_config);
    info!(
        "context_open_world_probe target_tile=({}, {})",
        target_tile.x, target_tile.y
    );
    for object in client_state.world_objects.values() {
        if object.tile_position != target_tile {
            continue;
        }

        let near = is_near_player(&player_position, &object.tile_position);
        let can_use = near && object_is_usable(object.object_id, &object_registry, &definitions);
        let interaction = if near {
            applicable_interaction(object, &definitions)
        } else {
            None
        };
        context_menu_state.show(
            cursor_position,
            ContextMenuTarget::World(object.object_id),
            near && object.is_container,
            can_use,
            near && can_use_on(
                object.object_id,
                &object_registry,
                &definitions,
                &spell_definitions,
            ),
            object.is_npc,
            near && object.quantity > 1,
            near && object.is_npc && object.has_dialog,
            interaction,
        );
        info!(
            "context_open_world_success object_id={} has_container={} can_use={} can_attack={} near={}",
            object.object_id, object.is_container, can_use, object.is_npc, near
        );
        return;
    }

    for remote_player in client_state.remote_players.values() {
        if remote_player.tile_position != target_tile {
            continue;
        }

        let near = is_near_player(&player_position, &remote_player.tile_position);
        let can_use =
            near && object_is_usable(remote_player.object_id, &object_registry, &definitions);
        context_menu_state.show(
            cursor_position,
            ContextMenuTarget::World(remote_player.object_id),
            false,
            can_use,
            near && can_use_on(
                remote_player.object_id,
                &object_registry,
                &definitions,
                &spell_definitions,
            ),
            true,
            false,
            false,
            None,
        );
        info!(
            "context_open_remote_player_success object_id={} can_use={} can_attack=true near={}",
            remote_player.object_id, can_use, near
        );
        return;
    }

    info!("context_open_no_target");
    context_menu_state.hide();
}

pub fn sync_docked_panel_layout(
    docked_panel_state: Res<DockedPanelState>,
    mut panel_queries: ParamSet<(
        Query<(&DockedPanelRoot, &mut Node, &mut Visibility), With<DockedPanelRoot>>,
        Query<(&DockedPanelCloseButton, &mut Visibility), With<DockedPanelCloseButton>>,
        Query<(&DockedPanelResizeHandle, &mut Visibility), With<DockedPanelResizeHandle>>,
    )>,
) {
    for (panel_root, mut node, mut visibility) in &mut panel_queries.p0() {
        if let Some(panel) = docked_panel_state.panel(panel_root.panel_id) {
            let top_offset = docked_panel_state
                .panels
                .iter()
                .take_while(|candidate| candidate.id != panel_root.panel_id)
                .map(|candidate| candidate.height + 8.0)
                .sum::<f32>();
            node.display = Display::Flex;
            node.height = px(panel.height);
            node.top = px(top_offset);
            *visibility = Visibility::Visible;
        } else {
            node.display = Display::None;
            node.top = px(0.0);
            *visibility = Visibility::Hidden;
        }
    }

    for (close_button, mut visibility) in &mut panel_queries.p1() {
        *visibility = if docked_panel_state
            .panel(close_button.panel_id)
            .is_some_and(|panel| panel.closable)
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    for (resize_handle, mut visibility) in &mut panel_queries.p2() {
        *visibility = if docked_panel_state
            .panel(resize_handle.panel_id)
            .is_some_and(|panel| panel.resizable)
        {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

pub fn sync_docked_panel_titles(
    client_state: Res<ClientGameState>,
    docked_panel_state: ResMut<DockedPanelState>,
    object_registry: Res<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    mut title_query: Query<(&DockedPanelTitle, &mut Text), With<DockedPanelTitle>>,
) {
    let mut docked_panel_state = docked_panel_state;

    for (title, mut text) in &mut title_query {
        let resolved_title = match docked_panel_state
            .panel(title.panel_id)
            .map(|panel| panel.kind)
        {
            Some(DockedPanelKind::Minimap) => "Minimap".to_owned(),
            Some(DockedPanelKind::Status) => "Status".to_owned(),
            Some(DockedPanelKind::Equipment) => "Equipment".to_owned(),
            Some(DockedPanelKind::Backpack) => "Backpack".to_owned(),
            Some(DockedPanelKind::CurrentTarget) => "Current Target".to_owned(),
            Some(DockedPanelKind::Container { object_id }) => client_state
                .world_objects
                .get(&object_id)
                .and_then(|_| {
                    object_registry.display_name(object_id, &definitions, &spell_definitions)
                })
                .unwrap_or_else(|| "Container".to_owned()),
            None => String::new(),
        };

        if let Some(panel) = docked_panel_state.panel_mut(title.panel_id) {
            panel.title = resolved_title.clone();
        }

        text.0 = resolved_title;
    }
}

pub fn sync_item_slot_button_visibility(
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    mut slot_button_query: Query<(&ItemSlotButton, &mut Visibility), With<ContainerSlotButton>>,
    mut backpack_row_query: Query<(&BackpackSlotRow, &mut Node)>,
) {
    let visible_backpack_capacity = client_state
        .inventory
        .backpack_slots
        .len()
        .min(client_state.player_storage_slots);

    for (slot, mut visibility) in &mut slot_button_query {
        let should_show = match slot.kind {
            ItemSlotKind::Backpack(index) => index < visible_backpack_capacity,
            ItemSlotKind::OpenContainer {
                panel_id,
                slot_index,
            } => docked_panel_state
                .container_object_id_for_panel(panel_id)
                .and_then(|object_id| client_state.container_slots.get(&object_id))
                .is_some_and(|slots| slot_index < slots.len()),
            ItemSlotKind::Equipment(_) => true,
        };

        *visibility = if should_show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    let visible_backpack_rows = visible_backpack_capacity.div_ceil(4);
    for (row, mut node) in &mut backpack_row_query {
        node.display = if row.row_index < visible_backpack_rows {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub fn sync_container_slot_images(
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    mut image_query: Query<
        (&ItemSlotImage, &mut ImageNode, &mut Visibility),
        With<ContainerSlotImage>,
    >,
    mut label_query: Query<
        (&ItemSlotQuantityLabel, &mut Text, &mut Visibility),
        Without<ContainerSlotImage>,
    >,
) {
    for (slot, mut image_node, mut visibility) in &mut image_query {
        let stack = match slot.kind {
            ItemSlotKind::Backpack(index) => client_state
                .inventory
                .backpack_slots
                .get(index)
                .cloned()
                .flatten(),
            ItemSlotKind::OpenContainer {
                panel_id,
                slot_index,
            } => docked_panel_state
                .container_object_id_for_panel(panel_id)
                .and_then(|object_id| client_state.container_slots.get(&object_id))
                .and_then(|slots| slots.get(slot_index).cloned().flatten()),
            ItemSlotKind::Equipment(_) => None,
        };
        let Some(stack) = stack else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(definition) = definitions.get(&stack.type_id) else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(sprite_path) = definition
            .sprite_for_count(stack.quantity)
            .map(str::to_owned)
        else {
            *visibility = Visibility::Hidden;
            continue;
        };

        image_node.image = asset_server.load(sprite_path);
        *visibility = Visibility::Visible;
    }

    for (label, mut text, mut visibility) in &mut label_query {
        let stack = match label.kind {
            ItemSlotKind::Backpack(index) => client_state
                .inventory
                .backpack_slots
                .get(index)
                .cloned()
                .flatten(),
            ItemSlotKind::OpenContainer {
                panel_id,
                slot_index,
            } => docked_panel_state
                .container_object_id_for_panel(panel_id)
                .and_then(|object_id| client_state.container_slots.get(&object_id))
                .and_then(|slots| slots.get(slot_index).cloned().flatten()),
            ItemSlotKind::Equipment(slot) => {
                if slot == crate::world::object_definitions::EquipmentSlot::Ammo {
                    client_state.inventory.ammo_stack()
                } else {
                    None
                }
            }
        };
        match stack {
            Some(s) if s.quantity > 1 => {
                text.0 = s.quantity.to_string();
                *visibility = Visibility::Visible;
            }
            _ => {
                text.0.clear();
                *visibility = Visibility::Hidden;
            }
        }
    }
}

pub fn sync_equipment_slot_images(
    client_state: Res<ClientGameState>,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    mut image_query: Query<
        (&ItemSlotImage, &mut ImageNode, &mut Visibility),
        With<EquipmentSlotImage>,
    >,
) {
    for (slot, mut image_node, mut visibility) in &mut image_query {
        let item = match slot.kind {
            ItemSlotKind::Equipment(slot) => client_state.inventory.equipment_item(slot).cloned(),
            ItemSlotKind::Backpack(_) | ItemSlotKind::OpenContainer { .. } => None,
        };
        let Some(item) = item else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(definition) = definitions.get(&item.type_id) else {
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

pub fn sync_context_menu_take_partial_button(
    context_menu_state: Res<ContextMenuState>,
    mut button_query: Query<&mut Node, With<ContextMenuTakePartialButton>>,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };
    node.display = if context_menu_state.can_take_partial {
        Display::Flex
    } else {
        Display::None
    };
}

pub fn update_take_partial_popup_visibility(
    take_partial_state: Res<TakePartialState>,
    mut popup_query: Query<&mut Visibility, With<TakePartialPopupRoot>>,
) {
    let Ok(mut vis) = popup_query.single_mut() else {
        return;
    };
    *vis = if take_partial_state.source.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

pub fn sync_take_partial_label(
    take_partial_state: Res<TakePartialState>,
    mut label_query: Query<&mut Text, With<TakePartialAmountLabel>>,
) {
    let Ok(mut text) = label_query.single_mut() else {
        return;
    };
    if take_partial_state.source.is_some() {
        text.0 = take_partial_state.selected_amount.to_string();
    }
}

pub fn handle_take_partial_buttons(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    mut take_partial_state: ResMut<TakePartialState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    dec_query: Query<(&ComputedNode, &UiGlobalTransform), With<TakePartialDecButton>>,
    inc_query: Query<(&ComputedNode, &UiGlobalTransform), With<TakePartialIncButton>>,
    confirm_query: Query<(&ComputedNode, &UiGlobalTransform), With<TakePartialConfirmButton>>,
    cancel_query: Query<(&ComputedNode, &UiGlobalTransform), With<TakePartialCancelButton>>,
) {
    if take_partial_state.source.is_none() {
        return;
    }
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    if is_cursor_over_button(cursor_position, &dec_query) {
        take_partial_state.selected_amount =
            take_partial_state.selected_amount.saturating_sub(1).max(1);
        return;
    }

    if is_cursor_over_button(cursor_position, &inc_query) {
        take_partial_state.selected_amount =
            (take_partial_state.selected_amount + 1).min(take_partial_state.max_amount);
        return;
    }

    if is_cursor_over_button(cursor_position, &confirm_query) {
        let Some(source) = take_partial_state.source else {
            return;
        };
        let amount = take_partial_state.selected_amount;
        let destination = find_best_take_destination(source, &client_state, &docked_panel_state);
        if let Some(destination) = destination {
            pending_commands.push(GameCommand::TakeFromStack {
                source,
                amount,
                destination,
            });
        }
        take_partial_state.source = None;
        return;
    }

    if is_cursor_over_button(cursor_position, &cancel_query) {
        take_partial_state.source = None;
    }
}

fn find_best_take_destination(
    source: ItemReference,
    client_state: &ClientGameState,
    _docked_panel_state: &DockedPanelState,
) -> Option<ItemDestination> {
    let visible = client_state
        .player_storage_slots
        .min(client_state.inventory.backpack_slots.len());
    // Find first empty backpack slot that isn't the source slot itself
    for i in 0..visible {
        if matches!(source, ItemReference::Slot(ItemSlotRef::Backpack(s)) if s == i) {
            continue;
        }
        if client_state
            .inventory
            .backpack_slots
            .get(i)
            .and_then(|s| s.as_ref())
            .is_none()
        {
            return Some(ItemDestination::Slot(ItemSlotRef::Backpack(i)));
        }
    }
    None
}

pub fn handle_movable_dragging(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    interaction_state: (Res<ContextMenuState>, Res<UseOnState>),
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    mut drag_state: ResMut<DragState>,
    mut pending_commands: ResMut<PendingGameCommands>,
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
) {
    let (context_menu_state, use_on_state) = interaction_state;
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Some(player_position) = client_state.player_tile_position else {
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
            if stack_in_slot_kind(&client_state, &docked_panel_state, slot_kind).is_some() {
                info!(
                    "drag_start ui_slot={slot_kind:?} open_containers={}",
                    docked_panel_state
                        .panels
                        .iter()
                        .filter(|panel| matches!(panel.kind, DockedPanelKind::Container { .. }))
                        .count()
                );
                info!(
                    "equipment_state_snapshot {}",
                    equipment_state_summary(&client_state.inventory)
                );
                drag_state.source = Some(DragSource::UiSlot(slot_kind));
                drag_state.object_id = None;
                drag_state.world_origin = None;
                return;
            }
        }

        let target_tile = cursor_to_tile(window, cursor_position, &player_position, &world_config);

        for object in client_state.world_objects.values() {
            if !object.is_movable || object.tile_position != target_tile {
                continue;
            }

            if !is_near_player(&player_position, &object.tile_position) {
                continue;
            }

            info!(
                "drag_start world_object_id={} origin=({}, {})",
                object.object_id, object.tile_position.x, object.tile_position.y
            );
            drag_state.source = Some(DragSource::World);
            drag_state.object_id = Some(object.object_id);
            drag_state.world_origin = Some(object.tile_position);
            break;
        }
    }

    if !mouse_input.just_released(MouseButton::Left) || drag_state.source.is_none() {
        return;
    }

    let target_tile = cursor_to_tile(window, cursor_position, &player_position, &world_config);
    let drag_source = drag_state.source.take();
    let dragged_object_id = drag_state.object_id.take();
    let world_origin = drag_state.world_origin.take();

    info!(
        "drag_release source={:?} object_id={:?} hovered_slot={hovered_slot:?} target_tile=({}, {})",
        drag_source.as_ref().map(drag_source_name),
        dragged_object_id,
        target_tile.x,
        target_tile.y
    );
    if hovered_slot.is_none() {
        log_equipment_slot_bounds(cursor_position, &slot_queries.p0());
    }

    match drag_source {
        Some(DragSource::World) => {
            let Some(object_id) = dragged_object_id else {
                return;
            };
            if let Some(slot_kind) = hovered_slot {
                if let Some(destination) = item_slot_kind_to_ref(slot_kind, &docked_panel_state) {
                    pending_commands.push(GameCommand::MoveItem {
                        source: ItemReference::WorldObject(object_id),
                        destination: ItemDestination::Slot(destination),
                    });
                    return;
                }
            }

            if world_origin.is_some() {
                pending_commands.push(GameCommand::MoveItem {
                    source: ItemReference::WorldObject(object_id),
                    destination: ItemDestination::WorldTile(target_tile),
                });
            }
        }
        Some(DragSource::UiSlot(source_slot)) => {
            let Some(source) = item_slot_kind_to_ref(source_slot, &docked_panel_state) else {
                return;
            };
            if let Some(slot_kind) = hovered_slot {
                if slot_kind == source_slot {
                    return;
                }

                if let Some(destination) = item_slot_kind_to_ref(slot_kind, &docked_panel_state) {
                    pending_commands.push(GameCommand::MoveItem {
                        source: ItemReference::Slot(source),
                        destination: ItemDestination::Slot(destination),
                    });
                    return;
                }
            }

            pending_commands.push(GameCommand::MoveItem {
                source: ItemReference::Slot(source),
                destination: ItemDestination::WorldTile(target_tile),
            });
        }
        None => {}
    }
}

pub fn sync_drag_preview(
    drag_state: Res<DragState>,
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
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

    let preview_label: Option<String> = match &drag_state.source {
        Some(DragSource::World) => drag_state.object_id.map(|object_id| {
            object_registry
                .display_name(object_id, &definitions, &spell_definitions)
                .unwrap_or_else(|| object_id.to_string())
        }),
        Some(DragSource::UiSlot(slot_kind)) => {
            stack_in_slot_kind(&client_state, &docked_panel_state, *slot_kind).map(|stack| {
                ObjectRegistry::display_name_for_type(
                    &stack.type_id,
                    Some(&stack.properties),
                    &definitions,
                    &spell_definitions,
                )
                .unwrap_or_else(|| stack.type_id.clone())
            })
        }
        None => None,
    };

    let Some(label_text) = preview_label else {
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
    label.0 = label_text;
}

pub fn handle_docked_panel_scrolling(
    mut mouse_wheel_reader: MessageReader<MouseWheel>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut body_query: Query<
        (
            &DockedPanelBody,
            &Node,
            &ComputedNode,
            &UiGlobalTransform,
            &mut ScrollPosition,
            &Visibility,
        ),
        With<DockedPanelBody>,
    >,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    for mouse_wheel in mouse_wheel_reader.read() {
        let mut delta_y = -mouse_wheel.y;
        if mouse_wheel.unit == MouseScrollUnit::Line {
            delta_y *= 21.0;
        }

        for (body, node, computed, global_transform, mut scroll_position, visibility) in
            &mut body_query
        {
            if *visibility == Visibility::Hidden {
                continue;
            }
            if !point_in_ui_node(cursor_position, computed, global_transform) {
                continue;
            }
            if node.overflow.y != bevy::ui::OverflowAxis::Scroll || delta_y == 0.0 {
                continue;
            }

            let max_offset =
                (computed.content_size() - computed.size()) * computed.inverse_scale_factor();
            if max_offset.y <= 0.0 {
                break;
            }

            scroll_position.y = (scroll_position.y + delta_y).clamp(0.0, max_offset.y);
            info!(
                "panel_scroll panel_id={} scroll_y={:.1}/{:.1}",
                body.panel_id, scroll_position.y, max_offset.y
            );
            break;
        }
    }
}

pub fn handle_docked_panel_dragging(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut drag_state: ResMut<DockedPanelDragState>,
    mut docked_panel_state: ResMut<DockedPanelState>,
    drag_handle_query: Query<
        (
            &DockedPanelDragHandle,
            &ComputedNode,
            &UiGlobalTransform,
            &Visibility,
        ),
        With<DockedPanelDragHandle>,
    >,
    panel_query: Query<
        (&DockedPanelRoot, &ComputedNode, &UiGlobalTransform),
        With<DockedPanelRoot>,
    >,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        if mouse_input.just_released(MouseButton::Left) {
            drag_state.panel_id = None;
        }
        return;
    };

    if mouse_input.just_pressed(MouseButton::Left) {
        for (handle, computed_node, global_transform, visibility) in &drag_handle_query {
            if *visibility == Visibility::Hidden {
                continue;
            }
            if !point_in_ui_node(cursor_position, computed_node, global_transform) {
                continue;
            }

            let Some(panel) = docked_panel_state.panel(handle.panel_id) else {
                continue;
            };
            if !panel.movable {
                continue;
            }

            drag_state.panel_id = Some(handle.panel_id);
            break;
        }
    }

    let Some(active_panel_id) = drag_state.panel_id else {
        return;
    };

    if mouse_input.just_released(MouseButton::Left) {
        drag_state.panel_id = None;
        return;
    }

    if !mouse_input.pressed(MouseButton::Left) {
        return;
    }

    let mut ordered_panels = docked_panel_state
        .panels
        .iter()
        .enumerate()
        .filter_map(|(index, panel)| {
            panel_query
                .iter()
                .find(|(root, _, _)| root.panel_id == panel.id)
                .map(|(_, computed, transform)| (index, panel.id, computed, transform))
        })
        .collect::<Vec<_>>();

    ordered_panels.sort_by_key(|(index, _, _, _)| *index);

    let mut target_index = 0usize;

    for (index, _, _computed, transform) in ordered_panels {
        let center_y = transform.translation.y;
        if cursor_position.y >= center_y {
            target_index = index + 1;
        }
    }

    let max_index = docked_panel_state.panels.len().saturating_sub(1);
    docked_panel_state.move_panel_to_index(active_panel_id, target_index.min(max_index));
}

pub fn handle_docked_panel_resizing(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut resize_state: ResMut<DockedPanelResizeState>,
    mut docked_panel_state: ResMut<DockedPanelState>,
    resize_handle_query: Query<
        (
            &DockedPanelResizeHandle,
            &ComputedNode,
            &UiGlobalTransform,
            &Visibility,
        ),
        With<DockedPanelResizeHandle>,
    >,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        if mouse_input.just_released(MouseButton::Left) {
            resize_state.panel_id = None;
        }
        return;
    };

    if mouse_input.just_pressed(MouseButton::Left) {
        for (handle, computed_node, global_transform, visibility) in &resize_handle_query {
            if *visibility == Visibility::Hidden {
                continue;
            }
            if !point_in_ui_node(cursor_position, computed_node, global_transform) {
                continue;
            }

            let Some(panel) = docked_panel_state.panel(handle.panel_id) else {
                continue;
            };
            if !panel.resizable {
                continue;
            }

            resize_state.panel_id = Some(handle.panel_id);
            resize_state.start_cursor_y = cursor_position.y;
            resize_state.start_height = panel.height;
            break;
        }
    }

    let Some(panel_id) = resize_state.panel_id else {
        return;
    };

    if mouse_input.just_released(MouseButton::Left) {
        resize_state.panel_id = None;
        return;
    }

    if !mouse_input.pressed(MouseButton::Left) {
        return;
    }

    let delta_y = cursor_position.y - resize_state.start_cursor_y;
    if let Some(panel) = docked_panel_state.panel_mut(panel_id) {
        panel.height = (resize_state.start_height + delta_y).clamp(
            DockedPanelState::MIN_PANEL_HEIGHT,
            DockedPanelState::MAX_PANEL_HEIGHT,
        );
    }
}

pub fn print_right_sidebar_layout_debug(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    sidebar_query: Query<
        (Entity, &ComputedNode, &UiGlobalTransform, &Children),
        With<RightSidebarRoot>,
    >,
    child_nodes: Query<(
        Option<&DockedPanelCanvas>,
        Option<&DockedPanelRoot>,
        &Node,
        &ComputedNode,
        &UiGlobalTransform,
        Option<&Visibility>,
    )>,
) {
    if !keyboard_input.just_pressed(KeyCode::F9) {
        return;
    }

    let Ok((sidebar_entity, computed_node, global_transform, children)) = sidebar_query.single()
    else {
        info!("layout_debug no_sidebar");
        return;
    };

    let sidebar_translation = global_transform.translation;
    info!(
        "layout_debug sidebar entity={sidebar_entity:?} pos=({:.1},{:.1}) size=({:.1},{:.1}) children={}",
        sidebar_translation.x,
        sidebar_translation.y,
        computed_node.size().x,
        computed_node.size().y,
        children.len()
    );

    for child in children.iter() {
        let Ok((canvas, docked, node, computed, transform, visibility)) = child_nodes.get(child)
        else {
            continue;
        };

        let label = if canvas.is_some() {
            "dock_canvas".to_owned()
        } else if let Some(docked) = docked {
            format!("docked_panel:{}", docked.panel_id)
        } else {
            "unknown".to_owned()
        };

        let translation = transform.translation;
        let visibility = visibility.copied().unwrap_or(Visibility::Inherited);
        info!(
            "layout_debug child={label} entity={child:?} display={:?} visibility={visibility:?} pos=({:.1},{:.1}) size=({:.1},{:.1}) flex_grow={:.1}",
            node.display,
            translation.x,
            translation.y,
            computed.size().x,
            computed.size().y,
            node.flex_grow
        );
    }
}

fn normalized_ratio(current: f32, maximum: f32) -> f32 {
    if maximum <= 0.0 {
        return 0.0;
    }

    (current / maximum).clamp(0.0, 1.0)
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

fn stack_in_slot_kind(
    client_state: &ClientGameState,
    docked_panel_state: &DockedPanelState,
    slot_kind: ItemSlotKind,
) -> Option<InventoryStack> {
    match slot_kind {
        ItemSlotKind::Backpack(slot_index) => client_state
            .inventory
            .backpack_slots
            .get(slot_index)
            .cloned()
            .flatten(),
        ItemSlotKind::OpenContainer {
            panel_id,
            slot_index,
        } => docked_panel_state
            .container_object_id_for_panel(panel_id)
            .and_then(|object_id| client_state.container_slots.get(&object_id))
            .and_then(|slots| slots.get(slot_index).cloned().flatten()),
        ItemSlotKind::Equipment(slot) => client_state.inventory.equipment_item(slot).map(|item| {
            let quantity = if slot == crate::world::object_definitions::EquipmentSlot::Ammo {
                client_state.inventory.ammo_quantity.max(1)
            } else {
                1
            };
            InventoryStack {
                type_id: item.type_id.clone(),
                properties: item.properties.clone(),
                quantity,
            }
        }),
    }
}

fn drag_source_name(source: &DragSource) -> &'static str {
    match source {
        DragSource::World => "world",
        DragSource::UiSlot(_) => "ui_slot",
    }
}

fn equipment_state_summary(inventory_state: &InventoryState) -> String {
    inventory_state
        .equipment_slots
        .iter()
        .map(|(slot, item)| {
            let item_label = item
                .as_ref()
                .map(|i| i.type_id.as_str())
                .unwrap_or("none");
            format!("{slot:?}={item_label}")
        })
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

/// Pick the first interaction whose `from`-state filter matches the object's
/// current replicated state. Returns `(verb, label)`. Used by the context
/// menu to surface a single dynamic-label "Interact" button.
fn applicable_interaction(
    object: &crate::game::resources::ClientWorldObjectState,
    definitions: &OverworldObjectDefinitions,
) -> Option<(String, String)> {
    let definition = definitions.get(&object.definition_id)?;
    if definition.interactions.is_empty() {
        return None;
    }
    let current_state = object.state.as_deref();
    let interaction = definition.interactions.iter().find(|i| {
        i.from.is_empty() || current_state.is_some_and(|cs| i.from.iter().any(|s| s == cs))
    })?;
    let label = interaction.label.clone().unwrap_or_else(|| {
        let mut chars = interaction.verb.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        }
    });
    Some((interaction.verb.clone(), label))
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

fn context_target_to_item_reference(
    target: ContextMenuTarget,
    docked_panel_state: &DockedPanelState,
) -> Option<ItemReference> {
    match target {
        ContextMenuTarget::World(object_id) => Some(ItemReference::WorldObject(object_id)),
        ContextMenuTarget::Slot(slot_kind) => {
            item_slot_kind_to_ref(slot_kind, docked_panel_state).map(ItemReference::Slot)
        }
    }
}

fn item_slot_kind_to_ref(
    slot_kind: ItemSlotKind,
    docked_panel_state: &DockedPanelState,
) -> Option<ItemSlotRef> {
    match slot_kind {
        ItemSlotKind::Backpack(slot_index) => Some(ItemSlotRef::Backpack(slot_index)),
        ItemSlotKind::Equipment(slot) => Some(ItemSlotRef::Equipment(slot)),
        ItemSlotKind::OpenContainer {
            panel_id,
            slot_index,
        } => Some(ItemSlotRef::Container {
            object_id: docked_panel_state.container_object_id_for_panel(panel_id)?,
            slot_index,
        }),
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
        player_position.z,
    )
}

fn is_near_player(player_position: &TilePosition, target_position: &TilePosition) -> bool {
    if player_position.z != target_position.z {
        return false;
    }
    let delta_x = (player_position.x - target_position.x).abs();
    let delta_y = (player_position.y - target_position.y).abs();

    delta_x <= 1 && delta_y <= 1
}
