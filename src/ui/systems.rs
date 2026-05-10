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
    DockedPanelRoot, DockedPanelTitle, DragPreviewImage, DragPreviewLabel, DragPreviewQuantity,
    DragPreviewRoot, EquipmentSlotButton, EquipmentSlotImage, HealthFill, ItemSlotButton,
    ItemSlotImage, ItemSlotKind, ItemSlotQuantityLabel, ItemTooltipLabel, ItemTooltipRoot,
    ManaFill, RightSidebarRoot, TakePartialAmountLabel, TakePartialCancelButton,
    TakePartialConfirmButton, TakePartialDecButton, TakePartialIncButton, TakePartialPopupRoot,
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
    mut trade_popup_state: ResMut<crate::ui::resources::TradePopupState>,
    mut dialog_state: ResMut<crate::ui::resources::ActiveDialogState>,
) {
    let events = std::mem::take(&mut pending_ui_events.events);

    for event in events {
        match event {
            GameUiEvent::OpenContainer { object_id } => {
                docked_panel_state.open(object_id);
            }
            GameUiEvent::OpenTradePanel { session_id } => {
                trade_popup_state.open(session_id);
            }
            GameUiEvent::CloseTradePanel { .. } => {
                trade_popup_state.close();
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
            other @ GameUiEvent::LevelUpToast { .. } => {
                pending_ui_events.events.push(other);
            }
            other @ GameUiEvent::DeathSummary { .. } => {
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
        // Pouch panels stay open as long as the underlying inventory slot is
        // still a container item. Slot empties / replaced with a non-pouch ->
        // close.
        DockedPanelKind::PouchInBackpack { backpack_slot } => client_state
            .inventory
            .backpack_slots
            .get(backpack_slot)
            .and_then(|slot| slot.as_ref())
            .is_some_and(|stack| stack.contained_slots.is_some()),
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
                Some(DockedPanelKind::PouchInBackpack { .. }) => {
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

/// Updates the status-panel weight readout from the replicated
/// `ClientCarryWeight`. Renders `Weight: 8.4 / 40 kg` plus an
/// "(Encumbered)" tag — color set per-frame to mark the encumbered state in
/// danger red.
pub fn sync_carry_weight_label(
    client_state: Res<ClientGameState>,
    palette: Res<crate::ui::theme::palette::Palette>,
    mut label_query: Query<
        (&mut Text, &mut TextColor),
        With<crate::ui::components::CarryWeightLabel>,
    >,
) {
    let Ok((mut text, mut color)) = label_query.single_mut() else {
        return;
    };
    let (new_text, new_color) = match client_state.carry_weight {
        Some(carry) if carry.soft_cap_kg > 0.0 => {
            let label = if carry.encumbered {
                format!(
                    "Weight: {:.1} / {:.0} kg (Encumbered)",
                    carry.current_kg, carry.soft_cap_kg
                )
            } else {
                format!(
                    "Weight: {:.1} / {:.0} kg",
                    carry.current_kg, carry.soft_cap_kg
                )
            };
            let c = if carry.encumbered {
                palette.text_danger
            } else {
                palette.text_value
            };
            (label, c)
        }
        _ => (String::new(), palette.text_value),
    };
    if text.0 != new_text {
        text.0 = new_text;
    }
    if color.0 != new_color {
        color.0 = new_color;
    }
}

/// Mirrors `sync_vital_bars` for the XP bar + level/XP label. Width is the
/// fraction of the *current level interval* completed (not lifetime XP). At
/// the cap the bar shows full and the label reads "Lv 20 (max)".
pub fn sync_xp_bar(
    client_state: Res<ClientGameState>,
    mut fill_query: Query<&mut Node, With<crate::ui::components::ExperienceFill>>,
    mut label_query: Query<
        &mut Text,
        (
            With<crate::ui::components::ExperienceLabel>,
            Without<crate::ui::components::ExperienceFill>,
        ),
    >,
) {
    let Some(view) = client_state.experience.as_ref() else {
        return;
    };

    let ratio = match view.xp_for_next {
        Some(span) if span > 0 => (view.xp_into_level as f32 / span as f32).clamp(0.0, 1.0),
        _ => 1.0,
    };
    for mut node in &mut fill_query {
        node.width = percent(ratio * 100.0);
    }

    let label = match view.xp_for_next {
        Some(span) => format!("Lv {} ({}/{})", view.level, view.xp_into_level, span),
        None => format!("Lv {} (max)", view.level),
    };
    for mut text in &mut label_query {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}

/// Spawns the class-picker fullscreen modal when `client_state.class_chosen`
/// is `false` and the local player is identified, despawns it when the
/// server confirms a choice.
pub fn manage_class_picker(
    client_state: Res<ClientGameState>,
    mut commands: Commands,
    overlays: Query<Entity, With<crate::ui::components::ClassPickerOverlay>>,
) {
    let needs_picker = client_state.local_player_id.is_some() && !client_state.class_chosen;
    let already_open = overlays.iter().next().is_some();

    if needs_picker && !already_open {
        spawn_class_picker_overlay(&mut commands);
    } else if !needs_picker && already_open {
        for entity in overlays.iter() {
            commands.entity(entity).despawn();
        }
    }
}

fn spawn_class_picker_overlay(commands: &mut Commands) {
    use crate::player::classes::Class;
    use crate::ui::components::{ClassPickerButton, ClassPickerOverlay};

    let class_blurbs: [(Class, &str, &str); 4] = [
        (
            Class::Fighter,
            "Fighter",
            "d10 HP. Front-line martial. Hits hard, soaks hits, doesn't cast.",
        ),
        (
            Class::Wizard,
            "Wizard",
            "d4 HP. Arcane caster - fragile, mana-rich, scales hard.",
        ),
        (
            Class::Cleric,
            "Cleric",
            "d8 HP. Divine caster - mid martial, full healer / support.",
        ),
        (
            Class::Vagabond,
            "Vagabond",
            "d6 HP. Skill specialist, opportunistic damage. 8 skill points / level.",
        ),
    ];

    commands
        .spawn((
            ClassPickerOverlay,
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                width: percent(100.0),
                height: percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.78)),
            GlobalZIndex(2000),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    width: px(420.0),
                    padding: UiRect::all(px(20.0)),
                    row_gap: px(12.0),
                    align_items: AlignItems::Stretch,
                    border: UiRect::all(px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.08, 0.04)),
                BorderColor::all(Color::srgb(0.86, 0.72, 0.32)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Choose your class"),
                    TextFont {
                        font_size: 22.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.86, 0.50)),
                ));
                panel.spawn((
                    Text::new(
                        "Your class shapes hit dice, mana scaling, and skill points.\n\
                         You can change later via admin REPL only.",
                    ),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.80, 0.76, 0.66)),
                ));
                for (class, name, blurb) in class_blurbs {
                    panel
                        .spawn((
                            Button,
                            ClassPickerButton { class },
                            Node {
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(10.0)),
                                row_gap: px(2.0),
                                border: UiRect::all(px(1.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.16, 0.13, 0.08)),
                            BorderColor::all(Color::srgb(0.48, 0.36, 0.22)),
                        ))
                        .with_children(|button| {
                            button.spawn((
                                Text::new(name),
                                TextFont {
                                    font_size: 18.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.96, 0.92, 0.80)),
                            ));
                            button.spawn((
                                Text::new(blurb),
                                TextFont {
                                    font_size: 12.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.78, 0.72, 0.62)),
                            ));
                        });
                }
            });
        });
}

/// Click on the floating player-sprite button: toggles the Character sheet
/// modal open/closed.
pub fn handle_character_sheet_button_click(
    interactions: Query<
        &Interaction,
        (
            Changed<Interaction>,
            With<crate::ui::components::CharacterSheetButton>,
        ),
    >,
    mut state: ResMut<crate::ui::resources::CharacterSheetState>,
) {
    if interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed))
    {
        state.open = !state.open;
    }
}

/// Click on the close button inside the Character sheet modal.
pub fn handle_character_sheet_close_click(
    interactions: Query<
        &Interaction,
        (
            Changed<Interaction>,
            With<crate::ui::components::CharacterSheetCloseButton>,
        ),
    >,
    mut state: ResMut<crate::ui::resources::CharacterSheetState>,
) {
    if interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed))
    {
        state.open = false;
    }
}

/// Spawns / despawns the Character sheet fullscreen modal based on
/// `CharacterSheetState.open`. Re-renders on every tick the modal is open
/// — cheap because the panel is small and rebuilds let it stay in sync
/// with `ClientGameState` without per-field change-detection plumbing.
pub fn manage_character_sheet_overlay(
    state: Res<crate::ui::resources::CharacterSheetState>,
    client_state: Res<ClientGameState>,
    overlays: Query<Entity, With<crate::ui::components::CharacterSheetOverlay>>,
    mut commands: Commands,
) {
    let already_open = overlays.iter().next().is_some();

    if !state.open {
        if already_open {
            for entity in overlays.iter() {
                commands.entity(entity).despawn();
            }
        }
        return;
    }

    // If state.open is true and we have content drift (e.g. attributes just
    // changed), respawn so the layout stays current.
    if already_open && !state.is_changed() && !client_state.is_changed() {
        return;
    }

    for entity in overlays.iter() {
        commands.entity(entity).despawn();
    }
    spawn_character_sheet_overlay(&mut commands, &client_state);
}

fn spawn_character_sheet_overlay(commands: &mut Commands, state: &ClientGameState) {
    use crate::player::classes::ability_mod;
    use crate::ui::components::{CharacterSheetCloseButton, CharacterSheetOverlay};

    let class_label = state.class.map(|c| c.label()).unwrap_or("Adventurer");
    let level_line = match &state.experience {
        Some(view) => match view.xp_for_next {
            Some(span) => format!(
                "Level {} {} - {}/{} XP",
                view.level, class_label, view.xp_into_level, span
            ),
            None => format!("Level {} {} - max level", view.level, class_label),
        },
        None => class_label.to_owned(),
    };

    commands
        .spawn((
            CharacterSheetOverlay,
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                width: percent(100.0),
                height: percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
            GlobalZIndex(1200),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    width: px(460.0),
                    padding: UiRect::all(px(20.0)),
                    row_gap: px(10.0),
                    align_items: AlignItems::Stretch,
                    border: UiRect::all(px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.08, 0.06)),
                BorderColor::all(Color::srgb(0.86, 0.72, 0.32)),
            ))
            .with_children(|panel| {
                // Title.
                panel.spawn((
                    Text::new("Character"),
                    TextFont {
                        font_size: 22.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.86, 0.50)),
                ));

                // Class + level + XP line.
                panel.spawn((
                    Text::new(level_line),
                    TextFont {
                        font_size: 15.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.92, 0.80)),
                ));

                // Vitals summary line.
                if let Some(v) = state.player_vitals {
                    panel.spawn((
                        Text::new(format!(
                            "HP {:.0} / {:.0}    MP {:.0} / {:.0}",
                            v.health, v.max_health, v.mana, v.max_mana
                        )),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.86, 0.82, 0.70)),
                    ));
                }

                // Attributes section header.
                panel.spawn((
                    Node {
                        margin: UiRect::top(px(6.0)),
                        ..default()
                    },
                    Text::new("Attributes"),
                    TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.78, 0.46)),
                ));

                if let Some(attrs) = state.attributes {
                    let rows: [(&str, i32); 6] = [
                        ("STR (Strength)", attrs.strength),
                        ("AGI (Agility)", attrs.agility),
                        ("CON (Constitution)", attrs.constitution),
                        ("WIL (Willpower)", attrs.willpower),
                        ("CHA (Charisma)", attrs.charisma),
                        ("FOC (Focus)", attrs.focus),
                    ];
                    for (label, value) in rows {
                        let modifier = ability_mod(value);
                        let mod_str = if modifier >= 0 {
                            format!("+{modifier}")
                        } else {
                            modifier.to_string()
                        };
                        panel
                            .spawn((
                                Node {
                                    flex_direction: FlexDirection::Row,
                                    justify_content: JustifyContent::SpaceBetween,
                                    column_gap: px(8.0),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Text::new(label),
                                    TextFont {
                                        font_size: 14.0,
                                        ..default()
                                    },
                                    TextColor(Color::srgb(0.80, 0.76, 0.66)),
                                ));
                                row.spawn((
                                    Text::new(format!("{value}  [{mod_str}]")),
                                    TextFont {
                                        font_size: 14.0,
                                        ..default()
                                    },
                                    TextColor(Color::srgb(0.96, 0.92, 0.80)),
                                ));
                            });
                    }
                } else {
                    panel.spawn((
                        Text::new("(loading...)"),
                        TextFont {
                            font_size: 13.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.66, 0.62, 0.54)),
                    ));
                }

                // Status effects section.
                panel.spawn((
                    Node {
                        margin: UiRect::top(px(6.0)),
                        ..default()
                    },
                    Text::new("Status Effects"),
                    TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.78, 0.46)),
                ));

                let effect_text = match state.regen_buff {
                    Some(buff) if buff.remaining_seconds > 0.0 => {
                        let total = buff.remaining_seconds.ceil() as i32;
                        let mins = total / 60;
                        let secs = total % 60;
                        format!(
                            "Well Fed - regen x{:.1} ({mins}:{secs:02} remaining)",
                            buff.multiplier
                        )
                    }
                    _ => "No active effects.".to_owned(),
                };
                panel.spawn((
                    Text::new(effect_text),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.86, 0.82, 0.70)),
                ));

                // Close button.
                panel
                    .spawn((
                        Button,
                        CharacterSheetCloseButton,
                        Node {
                            margin: UiRect::top(px(14.0)),
                            padding: UiRect::axes(px(14.0), px(6.0)),
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.18, 0.14, 0.10)),
                        BorderColor::all(Color::srgb(0.60, 0.45, 0.28)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new("Close"),
                            TextFont {
                                font_size: 16.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.96, 0.92, 0.80)),
                        ));
                    });
            });
        });
}

/// Drains `GameUiEvent::DeathSummary` events from `PendingGameUiEvents` and
/// spawns the post-death recap overlay listing dropped items + XP lost.
pub fn consume_death_summary_events(
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    mut commands: Commands,
    existing: Query<Entity, With<crate::ui::components::DeathSummaryOverlay>>,
) {
    let events = std::mem::take(&mut pending_ui_events.events);
    for event in events {
        match event {
            GameUiEvent::DeathSummary {
                items_dropped,
                xp_lost,
            } => {
                // Replace any existing overlay so a quick second death doesn't
                // stack two panels.
                for entity in existing.iter() {
                    commands.entity(entity).despawn();
                }
                spawn_death_summary_overlay(&mut commands, items_dropped, xp_lost);
            }
            other => pending_ui_events.events.push(other),
        }
    }
}

fn spawn_death_summary_overlay(
    commands: &mut Commands,
    items: Vec<crate::game::resources::InventoryStackSummary>,
    xp_lost: u64,
) {
    use crate::ui::components::{DeathSummaryDismissButton, DeathSummaryOverlay};

    commands
        .spawn((
            DeathSummaryOverlay,
            Node {
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                width: percent(100.0),
                height: percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
            GlobalZIndex(1500),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    width: px(420.0),
                    padding: UiRect::all(px(20.0)),
                    row_gap: px(8.0),
                    align_items: AlignItems::Stretch,
                    border: UiRect::all(px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.05, 0.05)),
                BorderColor::all(Color::srgb(0.78, 0.32, 0.30)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("You fell."),
                    TextFont {
                        font_size: 22.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.62, 0.50)),
                ));
                if xp_lost > 0 {
                    panel.spawn((
                        Text::new(format!("XP lost: {xp_lost}")),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.92, 0.86, 0.62)),
                    ));
                } else {
                    panel.spawn((
                        Text::new("No XP lost."),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.78, 0.74, 0.62)),
                    ));
                }

                if items.is_empty() {
                    panel.spawn((
                        Text::new("Your gear stayed with you."),
                        TextFont {
                            font_size: 13.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.78, 0.74, 0.62)),
                    ));
                } else {
                    panel.spawn((
                        Text::new("Items left on your corpse:"),
                        TextFont {
                            font_size: 13.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.86, 0.78, 0.66)),
                    ));
                    for item in items {
                        let line = if item.quantity > 1 {
                            format!("  - {} x{}", item.display_name, item.quantity)
                        } else {
                            format!("  - {}", item.display_name)
                        };
                        panel.spawn((
                            Text::new(line),
                            TextFont {
                                font_size: 12.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.92, 0.88, 0.74)),
                        ));
                    }
                }

                panel
                    .spawn((
                        Button,
                        DeathSummaryDismissButton,
                        Node {
                            margin: UiRect::top(px(12.0)),
                            padding: UiRect::axes(px(14.0), px(6.0)),
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.18, 0.12, 0.10)),
                        BorderColor::all(Color::srgb(0.60, 0.40, 0.32)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new("Continue"),
                            TextFont {
                                font_size: 16.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.96, 0.92, 0.80)),
                        ));
                    });
            });
        });
}

/// Click handler for the death-summary dismiss button: despawns the overlay.
pub fn handle_death_summary_dismiss(
    mut commands: Commands,
    interactions: Query<
        &Interaction,
        (
            Changed<Interaction>,
            With<crate::ui::components::DeathSummaryDismissButton>,
        ),
    >,
    overlays: Query<Entity, With<crate::ui::components::DeathSummaryOverlay>>,
) {
    let pressed = interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed));
    if !pressed {
        return;
    }
    for entity in overlays.iter() {
        commands.entity(entity).despawn();
    }
}

/// Click handler for class-picker buttons. Pushes `ChooseClass` and lets the
/// server flip `class_chosen` so `manage_class_picker` despawns the overlay.
pub fn handle_class_picker_clicks(
    mut interactions: Query<
        (&Interaction, &crate::ui::components::ClassPickerButton),
        Changed<Interaction>,
    >,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    for (interaction, button) in &mut interactions {
        if matches!(interaction, Interaction::Pressed) {
            pending_commands.push(GameCommand::ChooseClass {
                class: button.class,
            });
        }
    }
}

/// Drains `GameUiEvent::LevelUpToast` events from `PendingGameUiEvents`,
/// spawning one transient overlay node per event. Other UI-event variants are
/// preserved in the queue for downstream consumers (mirrors
/// `consume_projectile_events`).
pub fn consume_level_up_toasts(
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    mut commands: Commands,
) {
    let events = std::mem::take(&mut pending_ui_events.events);
    for event in events {
        match event {
            GameUiEvent::LevelUpToast { new_level } => {
                commands.spawn((
                    crate::ui::components::LevelUpToast {
                        remaining_seconds: 3.0,
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        top: percent(15.0),
                        left: percent(50.0),
                        margin: UiRect::left(px(-120.0)),
                        width: px(240.0),
                        padding: UiRect::all(px(12.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.10, 0.08, 0.04, 0.85)),
                    BorderColor::all(Color::srgb(0.86, 0.72, 0.32)),
                    GlobalZIndex(1000),
                    children![(
                        Text::new(format!("Level Up! Lv {}", new_level)),
                        TextFont {
                            font_size: 22.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.86, 0.50)),
                    )],
                ));
            }
            other => pending_ui_events.events.push(other),
        }
    }
}

/// Ticks the fade timer on each spawned `LevelUpToast` overlay. Despawns the
/// node when the timer reaches zero.
pub fn tick_level_up_toasts(
    time: Res<Time>,
    mut commands: Commands,
    mut toasts: Query<(Entity, &mut crate::ui::components::LevelUpToast)>,
) {
    let dt = time.delta_secs();
    for (entity, mut toast) in &mut toasts {
        toast.remaining_seconds -= dt;
        if toast.remaining_seconds <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

pub fn sync_regen_buff_label(
    client_state: Res<ClientGameState>,
    mut label_query: Query<&mut Text, With<crate::ui::components::RegenBuffLabel>>,
) {
    let Ok(mut text) = label_query.single_mut() else {
        return;
    };
    let new_text = match &client_state.regen_buff {
        Some(buff) if buff.remaining_seconds > 0.0 => {
            let total_seconds = buff.remaining_seconds.ceil() as i32;
            let mins = total_seconds / 60;
            let secs = total_seconds % 60;
            format!("Well Fed: {mins}:{secs:02} (x{:.1})", buff.multiplier)
        }
        _ => String::new(),
    };
    if text.0 != new_text {
        text.0 = new_text;
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

pub fn sync_context_menu_trade_button(
    context_menu_state: Res<ContextMenuState>,
    mut button_query: Query<&mut Node, With<crate::ui::components::ContextMenuTradeButton>>,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };

    node.display = if context_menu_state.can_trade {
        Display::Flex
    } else {
        Display::None
    };
}

/// "Offer to Trade" is shown when the right-clicked target is one of the
/// player's own backpack/equipment/pouch slots AND a trade panel is open.
pub fn sync_context_menu_offer_to_trade_button(
    context_menu_state: Res<ContextMenuState>,
    trade_popup_state: Res<crate::ui::resources::TradePopupState>,
    mut button_query: Query<
        &mut Node,
        With<crate::ui::components::ContextMenuOfferToTradeButton>,
    >,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };

    let target_is_player_slot = matches!(
        context_menu_state.target,
        Some(ContextMenuTarget::Slot(
            ItemSlotKind::Backpack(_)
                | ItemSlotKind::Equipment(_)
                | ItemSlotKind::PouchInBackpack { .. }
        ))
    );
    let trade_open = trade_popup_state.session_id.is_some();

    node.display = if target_is_player_slot && trade_open {
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
        ResMut<DockedPanelState>,
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
        mut docked_panel_state,
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
                ContextMenuTarget::Slot(kind) => {
                    item_slot_kind_to_ref(kind, &docked_panel_state).map(InspectTarget::SlotItem)
                }
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
        match context_menu_state.target {
            Some(ContextMenuTarget::World(object_id)) => {
                pending_commands.push(GameCommand::OpenContainer { object_id });
            }
            Some(ContextMenuTarget::Slot(ItemSlotKind::Backpack(backpack_slot))) => {
                // Inventory pouch: no server roundtrip needed — slots come
                // straight off `client_state.inventory` and the panel reads
                // through `pouch_backpack_slot_for_panel`.
                docked_panel_state.open_pouch(backpack_slot);
            }
            _ => {}
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

/// Trade-related context-menu buttons (Trade / Offer to Trade) live in a
/// separate system so the main context-menu handler stays under Bevy's
/// 8-arm `ParamSet` limit.
#[allow(clippy::too_many_arguments)]
pub fn handle_trade_context_menu_actions(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    trade_popup_state: Res<crate::ui::resources::TradePopupState>,
    mut context_menu_state: ResMut<ContextMenuState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    trade_button_query: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<crate::ui::components::ContextMenuTradeButton>,
    >,
    offer_button_query: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<crate::ui::components::ContextMenuOfferToTradeButton>,
    >,
) {
    if !mouse_input.just_pressed(MouseButton::Left) || !context_menu_state.is_visible() {
        return;
    }
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    if is_cursor_over_button(cursor_position, &trade_button_query) {
        if let Some(ContextMenuTarget::World(object_id)) = context_menu_state.target {
            // Resolve the right-click target to either a remote player or a
            // shopkeeper world object. The client knows because the projected
            // `ClientWorldObjectState.is_shopkeeper` flag is set on
            // shopkeeper NPCs.
            let target = if client_state.remote_players.values().any(|p| p.object_id == object_id) {
                crate::game::trade::TradeTarget::Player { object_id }
            } else if client_state
                .world_objects
                .get(&object_id)
                .is_some_and(|obj| obj.is_shopkeeper)
            {
                crate::game::trade::TradeTarget::Shopkeeper { object_id }
            } else {
                context_menu_state.hide();
                return;
            };
            pending_commands.push(GameCommand::InitiateTrade { target });
        }
        context_menu_state.hide();
        return;
    }

    if is_cursor_over_button(cursor_position, &offer_button_query) {
        if let (Some(session_id), Some(ContextMenuTarget::Slot(slot_kind))) = (
            trade_popup_state.session_id,
            context_menu_state.target,
        ) {
            if let Some(slot_ref) = item_slot_kind_to_ref(slot_kind, &docked_panel_state) {
                if let Some(stack) =
                    stack_in_slot_kind(&client_state, &docked_panel_state, slot_kind)
                {
                    pending_commands.push(GameCommand::OfferTradeItem {
                        session_id,
                        source: slot_ref,
                        quantity: stack.quantity.max(1),
                    });
                }
            }
        }
        context_menu_state.hide();
    }
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
            // "Open" enabled for *inventory* pouches (a Backpack slot whose
            // item carries `contained_slots`). Pouches inside world
            // containers and equipment-slot pouches are intentionally
            // skipped — open them by moving them to your backpack first.
            let can_open =
                matches!(slot_kind, ItemSlotKind::Backpack(_)) && stack.contained_slots.is_some();
            let stack_qty = stack.quantity;
            context_menu_state.show(
                cursor_position,
                ContextMenuTarget::Slot(slot_kind),
                can_open,
                can_use,
                has_use_on,
                false,
                stack_qty > 1,
                false,
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

    // Priority order at a tile: remote player > NPC (incl. shopkeeper) > other
    // world object. Without this sort, the HashMap iteration order would let a
    // pickup item under a villager swallow the right-click.
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
            // Trade with remote players is enabled when adjacent. Self-target
            // never reaches this loop (the local player isn't in
            // `client_state.remote_players`).
            near,
            None,
        );
        info!(
            "context_open_remote_player_success object_id={} can_use={} can_attack=true near={}",
            remote_player.object_id, can_use, near
        );
        return;
    }

    let mut best_object: Option<&crate::game::resources::ClientWorldObjectState> = None;
    for object in client_state.world_objects.values() {
        if object.tile_position != target_tile {
            continue;
        }
        let upgrade = match best_object {
            None => true,
            Some(current) => object.is_npc && !current.is_npc,
        };
        if upgrade {
            best_object = Some(object);
        }
    }

    if let Some(object) = best_object {
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
            // "Trade" is enabled when adjacent to a shopkeeper NPC.
            near && object.is_shopkeeper,
            interaction,
        );
        info!(
            "context_open_world_success object_id={} has_container={} can_use={} can_attack={} near={}",
            object.object_id, object.is_container, can_use, object.is_npc, near
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
            Some(DockedPanelKind::PouchInBackpack { backpack_slot }) => client_state
                .inventory
                .backpack_slots
                .get(backpack_slot)
                .and_then(|slot| slot.as_ref())
                .and_then(|stack| definitions.get(&stack.type_id))
                .map(|def| def.name.clone())
                .unwrap_or_else(|| "Pouch".to_owned()),
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
            } => {
                if let Some(object_id) = docked_panel_state.container_object_id_for_panel(panel_id)
                {
                    client_state
                        .container_slots
                        .get(&object_id)
                        .is_some_and(|slots| slot_index < slots.len())
                } else if let Some(backpack_slot) =
                    docked_panel_state.pouch_backpack_slot_for_panel(panel_id)
                {
                    inventory_pouch_capacity(client_state.as_ref(), backpack_slot)
                        .is_some_and(|cap| slot_index < cap)
                } else {
                    false
                }
            }
            ItemSlotKind::PouchInBackpack { .. } => false,
            ItemSlotKind::Equipment(_) => true,
            // Trade slots have their own panel; this query only sees
            // ContainerSlotButton-tagged entities, so this arm is never hit.
            ItemSlotKind::TradeUs { .. }
            | ItemSlotKind::TradeThem { .. }
            | ItemSlotKind::MerchantWare { .. } => false,
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
            } => {
                if let Some(object_id) = docked_panel_state.container_object_id_for_panel(panel_id)
                {
                    client_state
                        .container_slots
                        .get(&object_id)
                        .and_then(|slots| slots.get(slot_index).cloned().flatten())
                } else if let Some(backpack_slot) =
                    docked_panel_state.pouch_backpack_slot_for_panel(panel_id)
                {
                    inventory_pouch_sub_slot(client_state.as_ref(), backpack_slot, slot_index)
                } else {
                    None
                }
            }
            ItemSlotKind::PouchInBackpack { .. } => None,
            ItemSlotKind::Equipment(_) => None,
            ItemSlotKind::TradeUs { .. }
            | ItemSlotKind::TradeThem { .. }
            | ItemSlotKind::MerchantWare { .. } => None,
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
            } => {
                if let Some(object_id) = docked_panel_state.container_object_id_for_panel(panel_id)
                {
                    client_state
                        .container_slots
                        .get(&object_id)
                        .and_then(|slots| slots.get(slot_index).cloned().flatten())
                } else if let Some(backpack_slot) =
                    docked_panel_state.pouch_backpack_slot_for_panel(panel_id)
                {
                    inventory_pouch_sub_slot(client_state.as_ref(), backpack_slot, slot_index)
                } else {
                    None
                }
            }
            ItemSlotKind::PouchInBackpack { .. } => None,
            ItemSlotKind::Equipment(slot) => {
                if slot == crate::world::object_definitions::EquipmentSlot::Ammo {
                    client_state.inventory.ammo_stack()
                } else {
                    None
                }
            }
            ItemSlotKind::TradeUs { .. }
            | ItemSlotKind::TradeThem { .. }
            | ItemSlotKind::MerchantWare { .. } => None,
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
            ItemSlotKind::Backpack(_)
            | ItemSlotKind::OpenContainer { .. }
            | ItemSlotKind::PouchInBackpack { .. }
            | ItemSlotKind::TradeUs { .. }
            | ItemSlotKind::TradeThem { .. }
            | ItemSlotKind::MerchantWare { .. } => None,
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

#[allow(clippy::too_many_arguments)]
pub fn handle_movable_dragging(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    interaction_state: (Res<ContextMenuState>, Res<UseOnState>),
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    trade_popup_state: Res<crate::ui::resources::TradePopupState>,
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
        Query<
            (
                &ItemSlotButton,
                &ComputedNode,
                &UiGlobalTransform,
                Option<&Visibility>,
            ),
            (With<Button>, With<crate::ui::components::TradeSlotButton>),
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
    // When the trade popup is open, trade slots are layered above the docked
    // inventory sidebar — so a Them drop-zone can sit on top of a backpack
    // slot. Check the trade family first in that case; otherwise keep the
    // existing equipment-then-container priority.
    let hovered_slot = if trade_popup_state.session_id.is_some() {
        hovered_slot_in_family(cursor_position, &slot_queries.p2())
            .or_else(|| hovered_slot_in_family(cursor_position, &slot_queries.p0()))
            .or_else(|| hovered_slot_in_family(cursor_position, &slot_queries.p1()))
    } else {
        hovered_slot_in_family(cursor_position, &slot_queries.p0())
            .or_else(|| hovered_slot_in_family(cursor_position, &slot_queries.p1()))
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
            // Trade-specific drops take priority when the trade popup is
            // open. These three branches all `return` because the source
            // slot kinds involved (MerchantWare, TradeUs, TradeThem) don't
            // resolve to a real `ItemSlotRef` and would otherwise fall
            // through the existing `MoveItem` path as no-ops.
            if let Some(session_id) = trade_popup_state.session_id {
                if let ItemSlotKind::MerchantWare { ware_index } = source_slot {
                    if matches!(hovered_slot, Some(ItemSlotKind::TradeThem { .. })) {
                        pending_commands.push(GameCommand::BrowseShopBuy {
                            session_id,
                            ware_index,
                            quantity: 1,
                        });
                    }
                    return;
                }
                if let ItemSlotKind::TradeUs { index } = source_slot {
                    let dropped_on_self = matches!(
                        hovered_slot,
                        Some(ItemSlotKind::TradeUs { index: i }) if i == index
                    );
                    if !dropped_on_self {
                        pending_commands.push(GameCommand::WithdrawTradeItem {
                            session_id,
                            offer_index: index,
                        });
                    }
                    return;
                }
                if matches!(source_slot, ItemSlotKind::TradeThem { .. }) {
                    // Read-only on the partner's column.
                    return;
                }
                if matches!(hovered_slot, Some(ItemSlotKind::TradeUs { .. })) {
                    if let Some(slot_ref) =
                        item_slot_kind_to_ref(source_slot, &docked_panel_state)
                    {
                        let qty = stack_in_slot_kind(
                            &client_state,
                            &docked_panel_state,
                            source_slot,
                        )
                        .map(|stack| stack.quantity)
                        .unwrap_or(1)
                        .max(1);
                        pending_commands.push(GameCommand::OfferTradeItem {
                            session_id,
                            source: slot_ref,
                            quantity: qty,
                        });
                    }
                    return;
                }
            }

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
    asset_server: Res<AssetServer>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut preview_query: Query<(&mut Node, &mut Visibility), With<DragPreviewRoot>>,
    mut label_query: Query<
        &mut Text,
        (
            With<DragPreviewLabel>,
            Without<DragPreviewRoot>,
            Without<DragPreviewQuantity>,
        ),
    >,
    mut image_query: Query<
        (&mut ImageNode, &mut Visibility),
        (
            With<DragPreviewImage>,
            Without<DragPreviewRoot>,
            Without<DragPreviewQuantity>,
        ),
    >,
    mut quantity_query: Query<
        (&mut Text, &mut Visibility),
        (
            With<DragPreviewQuantity>,
            Without<DragPreviewRoot>,
            Without<DragPreviewLabel>,
            Without<DragPreviewImage>,
        ),
    >,
) {
    let Ok((mut preview_node, mut root_visibility)) = preview_query.single_mut() else {
        return;
    };
    let Ok(mut label) = label_query.single_mut() else {
        return;
    };
    let Ok((mut image_node, mut image_visibility)) = image_query.single_mut() else {
        return;
    };
    let Ok((mut quantity_text, mut quantity_visibility)) = quantity_query.single_mut() else {
        return;
    };

    let resolved: Option<(String, Option<String>, u32)> = match &drag_state.source {
        Some(DragSource::World) => drag_state.object_id.and_then(|object_id| {
            let type_id = object_registry.type_id(object_id)?.to_owned();
            let properties = object_registry.properties(object_id);
            let name = ObjectRegistry::display_name_for_type(
                &type_id,
                properties,
                &definitions,
                &spell_definitions,
            )
            .unwrap_or_else(|| type_id.clone());
            let state = properties
                .and_then(|props| props.get("state"))
                .map(String::as_str);
            let sprite_path = definitions
                .get(&type_id)
                .and_then(|def| def.sprite_path_for_state(state).map(str::to_owned));
            Some((name, sprite_path, 1))
        }),
        Some(DragSource::UiSlot(slot_kind)) => {
            stack_in_slot_kind(&client_state, &docked_panel_state, *slot_kind).map(|stack| {
                let name = ObjectRegistry::display_name_for_type(
                    &stack.type_id,
                    Some(&stack.properties),
                    &definitions,
                    &spell_definitions,
                )
                .unwrap_or_else(|| stack.type_id.clone());
                let sprite_path = definitions
                    .get(&stack.type_id)
                    .and_then(|def| def.sprite_for_count(stack.quantity))
                    .map(str::to_owned);
                (name, sprite_path, stack.quantity)
            })
        }
        None => None,
    };

    let Some((label_text, sprite_path, quantity)) = resolved else {
        *root_visibility = Visibility::Hidden;
        *image_visibility = Visibility::Hidden;
        *quantity_visibility = Visibility::Hidden;
        label.0.clear();
        quantity_text.0.clear();
        return;
    };

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        *root_visibility = Visibility::Hidden;
        *image_visibility = Visibility::Hidden;
        *quantity_visibility = Visibility::Hidden;
        label.0.clear();
        quantity_text.0.clear();
        return;
    };

    *root_visibility = Visibility::Visible;
    preview_node.left = px(cursor_position.x + 14.0);
    preview_node.top = px(cursor_position.y + 14.0);
    label.0 = label_text;

    if let Some(sprite_path) = sprite_path {
        image_node.image = asset_server.load(sprite_path);
        *image_visibility = Visibility::Visible;
    } else {
        *image_visibility = Visibility::Hidden;
    }

    if quantity > 1 {
        quantity_text.0 = quantity.to_string();
        *quantity_visibility = Visibility::Visible;
    } else {
        quantity_text.0.clear();
        *quantity_visibility = Visibility::Hidden;
    }
}

pub fn sync_item_tooltip(
    drag_state: Res<DragState>,
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    slot_query: Query<(&Interaction, &ItemSlotButton)>,
    mut tooltip_query: Query<(&mut Node, &mut Visibility), With<ItemTooltipRoot>>,
    mut label_query: Query<&mut Text, (With<ItemTooltipLabel>, Without<ItemTooltipRoot>)>,
) {
    let Ok((mut tooltip_node, mut tooltip_visibility)) = tooltip_query.single_mut() else {
        return;
    };
    let Ok(mut label) = label_query.single_mut() else {
        return;
    };

    let hide = |tooltip_visibility: &mut Visibility, label: &mut Text| {
        *tooltip_visibility = Visibility::Hidden;
        label.0.clear();
    };

    if drag_state.source.is_some() {
        hide(&mut tooltip_visibility, &mut label);
        return;
    }

    let Some(hovered_slot) = slot_query
        .iter()
        .find(|(interaction, _)| matches!(interaction, Interaction::Hovered | Interaction::Pressed))
        .map(|(_, button)| button.kind)
    else {
        hide(&mut tooltip_visibility, &mut label);
        return;
    };

    let Some(stack) = stack_in_slot_kind(&client_state, &docked_panel_state, hovered_slot) else {
        hide(&mut tooltip_visibility, &mut label);
        return;
    };

    let name = ObjectRegistry::display_name_for_type(
        &stack.type_id,
        Some(&stack.properties),
        &definitions,
        &spell_definitions,
    )
    .unwrap_or_else(|| stack.type_id.clone());

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        hide(&mut tooltip_visibility, &mut label);
        return;
    };

    *tooltip_visibility = Visibility::Visible;
    tooltip_node.left = px(cursor_position.x + 18.0);
    tooltip_node.top = px(cursor_position.y - 24.0);
    label.0 = name;
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
        } => {
            if let Some(object_id) = docked_panel_state.container_object_id_for_panel(panel_id) {
                client_state
                    .container_slots
                    .get(&object_id)
                    .and_then(|slots| slots.get(slot_index).cloned().flatten())
            } else if let Some(backpack_slot) =
                docked_panel_state.pouch_backpack_slot_for_panel(panel_id)
            {
                inventory_pouch_sub_slot(client_state, backpack_slot, slot_index)
            } else {
                None
            }
        }
        ItemSlotKind::PouchInBackpack {
            panel_id,
            sub_slot_index,
        } => docked_panel_state
            .pouch_backpack_slot_for_panel(panel_id)
            .and_then(|backpack_slot| {
                inventory_pouch_sub_slot(client_state, backpack_slot, sub_slot_index)
            }),
        ItemSlotKind::Equipment(slot) => client_state.inventory.equipment_item(slot).map(|item| {
            let quantity = if slot == crate::world::object_definitions::EquipmentSlot::Ammo {
                client_state.inventory.ammo_quantity.max(1)
            } else {
                1
            };
            InventoryStack::item(item.type_id.clone(), item.properties.clone(), quantity)
        }),
        ItemSlotKind::TradeUs { index } => client_state
            .current_trade
            .as_ref()
            .and_then(|view| view.our_offers.get(index))
            .map(|entry| {
                InventoryStack::item(
                    entry.type_id.clone(),
                    entry.properties.clone(),
                    entry.quantity,
                )
            }),
        ItemSlotKind::TradeThem { index } => client_state
            .current_trade
            .as_ref()
            .and_then(|view| view.their_offers.get(index))
            .map(|entry| {
                InventoryStack::item(
                    entry.type_id.clone(),
                    entry.properties.clone(),
                    entry.quantity,
                )
            }),
        // Merchant wares aren't player inventory but the drag system probes
        // `stack_in_slot_kind` to decide whether the source slot is
        // "occupied" / draggable. Return a synthetic stack (quantity=1) so
        // the drag handshake passes.
        ItemSlotKind::MerchantWare { ware_index } => client_state
            .current_trade
            .as_ref()
            .and_then(|view| view.wares.as_ref())
            .and_then(|wares| wares.get(ware_index))
            .map(|ware| {
                InventoryStack::item(
                    ware.type_id.clone(),
                    crate::world::map_layout::ObjectProperties::new(),
                    1,
                )
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
            let item_label = item.as_ref().map(|i| i.type_id.as_str()).unwrap_or("none");
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
        } => {
            if let Some(object_id) = docked_panel_state.container_object_id_for_panel(panel_id) {
                Some(ItemSlotRef::Container {
                    object_id,
                    slot_index,
                })
            } else {
                let backpack_slot = docked_panel_state.pouch_backpack_slot_for_panel(panel_id)?;
                Some(ItemSlotRef::PouchInBackpack {
                    backpack_slot,
                    sub_slot: slot_index,
                })
            }
        }
        ItemSlotKind::PouchInBackpack {
            panel_id,
            sub_slot_index,
        } => Some(ItemSlotRef::PouchInBackpack {
            backpack_slot: docked_panel_state.pouch_backpack_slot_for_panel(panel_id)?,
            sub_slot: sub_slot_index,
        }),
        // Trade slots are not addressable as inventory locations — they're
        // a UI-side projection of `TradeOfferEntry`. Returning `None` here
        // keeps drag/drop and the context-menu inspect code from trying to
        // resolve a trade slot to a real inventory ref.
        ItemSlotKind::TradeUs { .. }
        | ItemSlotKind::TradeThem { .. }
        | ItemSlotKind::MerchantWare { .. } => None,
    }
}

/// Capacity of an inventory pouch at `backpack_slot`, or `None` if the slot
/// is empty / its item is not a container. Treats a `Some(stack)` whose
/// `contained_slots` is `None` (legacy data) as a zero-slot pouch.
fn inventory_pouch_capacity(client_state: &ClientGameState, backpack_slot: usize) -> Option<usize> {
    let stack = client_state
        .inventory
        .backpack_slots
        .get(backpack_slot)?
        .as_ref()?;
    stack.contained_slots.as_ref().map(|slots| slots.len())
}

/// Read the contents of an inventory pouch sub-slot. Returns `None` for
/// slots that are out of range, slots whose parent stack disappeared, or
/// slots in items that no longer carry `contained_slots`.
fn inventory_pouch_sub_slot(
    client_state: &ClientGameState,
    backpack_slot: usize,
    sub_slot: usize,
) -> Option<InventoryStack> {
    client_state
        .inventory
        .backpack_slots
        .get(backpack_slot)?
        .as_ref()?
        .contained_slots
        .as_ref()?
        .get(sub_slot)
        .cloned()
        .flatten()
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
