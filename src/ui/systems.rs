use bevy::ecs::query::QueryFilter;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::log::info;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow};

use crate::game::commands::{
    GameCommand, InspectTarget, ItemDestination, ItemReference, ItemSlotRef, UseTarget,
};
use crate::game::helpers::is_near_player;
use crate::game::resources::{
    ClientGameState, GameUiEvent, InventoryState, PendingGameCommands, PendingGameUiEvents,
};
use crate::magic::resources::{SpellDefinitions, SpellTargeting};
use crate::player::components::InventoryStack;
use crate::scripting::resources::PythonConsoleState;
use crate::ui::components::{
    BackpackSlotRow, ChatTerminal, ContainerSlotButton, ContainerSlotImage,
    ContextMenuAttackButton, ContextMenuInspectButton, ContextMenuOpenButton, ContextMenuRoot,
    ContextMenuTakePartialButton, ContextMenuUseButton, ContextMenuUseOnButton, DockedPanelBody,
    DockedPanelCanvas, DockedPanelCloseButton, DockedPanelDragHandle, DockedPanelResizeHandle,
    DockedPanelRoot, DockedPanelTitle, DragPreviewImage, DragPreviewLabel, DragPreviewQuantity,
    DragPreviewRoot, EquipmentSlotButton, EquipmentSlotImage, EquipmentSlotLabel, HealthFill,
    ItemSlotButton, ItemSlotImage, ItemSlotKind, ItemSlotQuantityLabel, ItemTooltipLabel,
    ItemTooltipRoot, JumpInfoBoxLabel, JumpInfoBoxRoot, JumpTileHighlight, ManaFill, NearbyNpcDot,
    NearbyNpcHpFill, NearbyNpcRow, NearbyNpcsList, QuickbarSlotMarker, RightSidebarRoot,
    TakePartialAmountLabel, TakePartialCancelButton, TakePartialConfirmButton,
    TakePartialDecButton, TakePartialIncButton, TakePartialPopupRoot,
};
use crate::ui::resources::{
    ContextMenuState, ContextMenuTarget, CursorMode, CursorState, DockedPanelDragState,
    DockedPanelKind, DockedPanelResizeState, DockedPanelState, DragSource, DragState, HoveredTile,
    ItemTargetingState, Quickbar, ShowCoordinates, SpellTargetingState, TakePartialState,
    UseOnState,
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
    mut book_panel_state: ResMut<crate::ui::book_panel::BookPanelState>,
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
            other @ GameUiEvent::VfxSpawn { .. } => {
                pending_ui_events.events.push(other);
            }
            // Floating speech bubbles are consumed by the speech_bubble
            // client-effects system. Re-queue so it sees the event.
            other @ GameUiEvent::SpeechBubble { .. } => {
                pending_ui_events.events.push(other);
            }
            // Recipe UI events are consumed by recipe-book systems
            // registered in `CraftingClientPlugin` (Step 6). Re-queue
            // them so those systems see them this frame.
            other @ GameUiEvent::RecipeLearnedToast { .. } => {
                pending_ui_events.events.push(other);
            }
            other @ GameUiEvent::OpenRecipeBook { .. } => {
                pending_ui_events.events.push(other);
            }
            // Skills events: consumed downstream by the skills-panel
            // systems. Re-queue them so those systems see them this frame.
            other @ GameUiEvent::OpenSkillsPanel => {
                pending_ui_events.events.push(other);
            }
            other @ GameUiEvent::SkillPointsToast { .. } => {
                pending_ui_events.events.push(other);
            }
            // Combat-feedback signals: hook point for future floating-text /
            // popup animation. The chat log already narrates these (see
            // `resolve_battle_turn`), so silent-consume is fine for now.
            GameUiEvent::AttackDodged { .. } | GameUiEvent::AttackBlocked { .. } => {}
            GameUiEvent::OpenBookPanel {
                source,
                kind,
                title,
                text,
                author_name,
                can_edit,
            } => {
                book_panel_state.open(source, kind, title, text, author_name, can_edit);
            }
        }
    }
}

pub fn toggle_cursor_mode(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    keybindings: Res<crate::ui::settings::Keybindings>,
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

    use crate::ui::settings::model::Action;
    if keybindings.just_pressed(Action::CursorUseOnToggle, &keyboard_input) {
        cursor_state.mode = match cursor_state.mode {
            CursorMode::Default => CursorMode::UseOn,
            CursorMode::UseOn => CursorMode::Default,
            CursorMode::SpellTarget => CursorMode::SpellTarget,
            CursorMode::SpellTargetTile => CursorMode::SpellTargetTile,
            CursorMode::ItemTarget => CursorMode::ItemTarget,
            CursorMode::AttackTarget => CursorMode::AttackTarget,
            CursorMode::JumpTarget => CursorMode::JumpTarget,
        };
    }

    if keybindings.just_pressed(Action::CursorAttackToggle, &keyboard_input) {
        cursor_state.mode = match cursor_state.mode {
            CursorMode::AttackTarget => CursorMode::Default,
            _ => CursorMode::AttackTarget,
        };
    }

    if keybindings.just_pressed(Action::Jump, &keyboard_input) {
        cursor_state.mode = match cursor_state.mode {
            CursorMode::JumpTarget => CursorMode::Default,
            _ => CursorMode::JumpTarget,
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
        .insert(cursor_icon_for_state(
            CursorMode::Default,
            None,
            &asset_server,
        ));
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

    *cursor_icon = cursor_icon_for_state(
        cursor_state.mode,
        cursor_state.use_on_sprite.as_deref(),
        &asset_server,
    );
}

/// Pick the sprite to show while the player is "Use On"-targeting with the
/// item identified by `source_type_id`. Returns `None` to mean "default
/// use_on_cursor.png". Resolution order: authored `use_on_cursor` on the
/// definition → gather sprite when the item is referenced by any tool_gate →
/// fall through.
fn resolve_use_on_sprite(
    source_type_id: &str,
    definitions: &OverworldObjectDefinitions,
) -> Option<String> {
    let definition = definitions.get(source_type_id)?;
    if let Some(custom) = definition.use_on_cursor.as_ref() {
        return Some(custom.clone());
    }
    if definitions.is_gathering_tool(source_type_id) {
        return Some("cursors/gather_cursor.png".to_owned());
    }
    None
}

fn cursor_icon_for_state(
    cursor_mode: CursorMode,
    use_on_sprite_override: Option<&str>,
    asset_server: &AssetServer,
) -> CursorIcon {
    let asset_path: String = match cursor_mode {
        CursorMode::Default => "cursors/default_cursor.png".to_owned(),
        CursorMode::UseOn => use_on_sprite_override
            .map(str::to_owned)
            .unwrap_or_else(|| "cursors/use_on_cursor.png".to_owned()),
        CursorMode::SpellTarget => "cursors/spell_target_cursor.png".to_owned(),
        CursorMode::SpellTargetTile => "cursors/spell_target_cursor.png".to_owned(),
        CursorMode::ItemTarget => "cursors/spell_target_cursor.png".to_owned(),
        CursorMode::AttackTarget => "cursors/attack_cursor.png".to_owned(),
        CursorMode::JumpTarget => "cursors/spell_target_cursor.png".to_owned(),
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
        | DockedPanelKind::NearbyNpcs => true,
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
                Some(DockedPanelKind::Container { object_id }) => {
                    pending_commands.push(GameCommand::CloseContainer { object_id });
                    docked_panel_state.close_panel(button.panel_id);
                }
                Some(DockedPanelKind::PouchInBackpack { .. }) => {
                    docked_panel_state.close_panel(button.panel_id);
                }
                // Menu-bar-toggleable singletons — close-X just removes
                // the docked row; the View menu re-opens it.
                Some(DockedPanelKind::Status)
                | Some(DockedPanelKind::Equipment)
                | Some(DockedPanelKind::Backpack)
                | Some(DockedPanelKind::Minimap)
                | Some(DockedPanelKind::NearbyNpcs) => {
                    docked_panel_state.close_panel(button.panel_id);
                }
                None => {}
            }
            return;
        }
    }
}

/// Tile distance used to sort the Nearby NPCs panel — Chebyshev (8-directional)
/// matches how server-side NPC AI measures range.
fn chebyshev_distance(a: TilePosition, b: TilePosition) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

/// Threat tier for the Nearby NPCs sort key. Lower is "more dangerous" so the
/// `sort_by_key` puts threats at the top: 0 = aggroed onto you, 1 = hostile
/// but not engaged with you, 2 = passive.
fn nearby_npc_threat_tier(o: &crate::game::resources::ClientWorldObjectState) -> u8 {
    if o.is_targeting_local_player {
        0
    } else if o.is_hostile {
        1
    } else {
        2
    }
}

fn nearby_npc_dot_asset(o: &crate::game::resources::ClientWorldObjectState) -> &'static str {
    if o.is_targeting_local_player {
        "ui/hud_indicators/dot_red.png"
    } else if o.is_hostile {
        "ui/hud_indicators/dot_yellow.png"
    } else {
        "ui/hud_indicators/dot_green.png"
    }
}

fn hp_fill_color(ratio: f32) -> Color {
    if ratio > 0.6 {
        Color::srgb(0.30, 0.78, 0.32)
    } else if ratio > 0.3 {
        Color::srgb(0.92, 0.70, 0.20)
    } else {
        Color::srgb(0.88, 0.25, 0.22)
    }
}

/// Two-phase reconciler for the Nearby NPCs panel:
///   - Phase A: when the sorted (threat_tier, distance) sequence of object_ids
///     changes, despawn all rows and respawn them in the new order. The
///     `Local<Vec<u64>>` snapshot avoids the despawn churn on frames where
///     nothing structurally changed.
///   - Phase B: every frame, refresh per-row visual state (dot color, HP fill,
///     target border) so HP ticks and aggro flips show up without rebuilds.
#[allow(clippy::too_many_arguments)]
pub fn sync_nearby_npcs_panel(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    client_state: Res<ClientGameState>,
    object_registry: Res<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    palette: Res<crate::ui::theme::Palette>,
    gameplay_settings: Option<Res<crate::ui::settings::gameplay::GameplaySettings>>,
    mut docked_panel_state: ResMut<DockedPanelState>,
    list_query: Query<Entity, With<NearbyNpcsList>>,
    row_query: Query<Entity, With<NearbyNpcRow>>,
    mut dot_query: Query<(&NearbyNpcDot, &mut ImageNode)>,
    mut hp_query: Query<(&NearbyNpcHpFill, &mut Node, &mut BackgroundColor)>,
    mut border_query: Query<(&NearbyNpcRow, &mut BorderColor)>,
    mut last_order: Local<Vec<u64>>,
    mut last_list_count: Local<usize>,
) {
    // Without a known player tile we can't sort by distance — but the auto-open
    // logic below still needs to run, so use a zero origin as a degenerate
    // fallback and accept that the order is meaningless on the first frame
    // before `PlayerPositionChanged` lands.
    let player_tile = client_state
        .player_tile_position
        .unwrap_or_else(|| TilePosition::new(0, 0, 0));
    // Show only NPCs within (their effective inspect range + 2) Chebyshev tiles
    // of the local player — keeps the list focused on NPCs the player could
    // actually interact with, plus a small buffer so a stepping NPC doesn't
    // flicker off the list at the edge.
    let nearby_window = |npc: &crate::game::resources::ClientWorldObjectState| -> i32 {
        definitions
            .get(&npc.definition_id)
            .and_then(|def| def.inspect_range)
            .unwrap_or(crate::world::hidden::DEFAULT_INSPECT_RANGE)
            + 2
    };
    // The scanner senses only the player's own floor by default — an enemy a
    // floor below or above stays hidden here (the minimap already floor-filters
    // its dots the same way). A future "sense other floors" skill would widen
    // this predicate.
    let player_floor = crate::world::components::floor_index(player_tile.z);
    let mut npcs: Vec<&crate::game::resources::ClientWorldObjectState> = client_state
        .world_objects
        .values()
        .filter(|o| o.is_npc)
        .filter(|o| crate::world::components::floor_index(o.tile_position.z) == player_floor)
        .filter(|o| chebyshev_distance(player_tile, o.tile_position) <= nearby_window(o))
        .collect();
    npcs.sort_by_key(|o| {
        (
            nearby_npc_threat_tier(o),
            chebyshev_distance(player_tile, o.tile_position),
        )
    });

    // Auto open/close: only when the user opted in via the Gameplay setting.
    let auto_open = gameplay_settings
        .as_deref()
        .map(|s| s.auto_open_nearby_npcs_panel)
        .unwrap_or(false);
    if auto_open {
        if npcs.is_empty() {
            docked_panel_state.close_nearby_npcs();
        } else {
            docked_panel_state.open_nearby_npcs();
        }
    }

    // Both the pre-spawned docked panel body and a freshly-undocked floating
    // window carry their own `NearbyNpcsList` child (each call to
    // `spawn_nearby_npcs_panel_body` adds one). Iterate over every instance so
    // the floating window also gets rows; force a rebuild when the count
    // changes so a newly-spawned floating list populates immediately even if
    // the NPC order is unchanged.
    let list_entities: Vec<Entity> = list_query.iter().collect();
    let current_order: Vec<u64> = npcs.iter().map(|o| o.object_id).collect();
    let order_changed = *last_order != current_order || list_entities.len() != *last_list_count;

    if order_changed {
        for row in row_query.iter() {
            commands.entity(row).despawn();
        }
        for list_entity in &list_entities {
            commands.entity(*list_entity).with_children(|parent| {
                for npc in &npcs {
                    let name = object_registry
                        .display_name(npc.object_id, &definitions, &spell_definitions)
                        .unwrap_or_else(|| npc.object_id.to_string());
                    spawn_nearby_npc_row(parent, &asset_server, &palette, npc, name);
                }
            });
        }
        *last_order = current_order;
        *last_list_count = list_entities.len();
    }

    let target = client_state.current_target_object_id;

    for (dot, mut image) in dot_query.iter_mut() {
        if let Some(npc) = client_state.world_objects.get(&dot.object_id) {
            let desired: Handle<Image> = asset_server.load(nearby_npc_dot_asset(npc));
            if image.image != desired {
                image.image = desired;
            }
        }
    }

    for (hp, mut node, mut bg) in hp_query.iter_mut() {
        if let Some(npc) = client_state.world_objects.get(&hp.object_id) {
            let (ratio, has_vitals) = match npc.vitals {
                Some(v) if v.max_health > 0.0 => ((v.health / v.max_health).clamp(0.0, 1.0), true),
                _ => (0.0, false),
            };
            node.width = percent(ratio * 100.0);
            bg.0 = if has_vitals {
                hp_fill_color(ratio)
            } else {
                Color::NONE
            };
        }
    }

    for (row, mut border) in border_query.iter_mut() {
        let is_target = Some(row.object_id) == target;
        *border = BorderColor::all(if is_target {
            palette.border_danger
        } else {
            Color::NONE
        });
    }
}

fn spawn_nearby_npc_row(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    palette: &crate::ui::theme::Palette,
    npc: &crate::game::resources::ClientWorldObjectState,
    name: String,
) {
    let object_id = npc.object_id;
    let dot_handle: Handle<Image> = asset_server.load(nearby_npc_dot_asset(npc));
    let (hp_ratio, has_vitals) = match npc.vitals {
        Some(v) if v.max_health > 0.0 => ((v.health / v.max_health).clamp(0.0, 1.0), true),
        _ => (0.0, false),
    };
    let hp_fill_bg = if has_vitals {
        hp_fill_color(hp_ratio)
    } else {
        Color::NONE
    };

    parent
        .spawn((
            Button,
            NearbyNpcRow { object_id },
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6.0),
                padding: UiRect::all(px(3.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
        ))
        .with_children(|row| {
            row.spawn((
                ImageNode::new(dot_handle),
                NearbyNpcDot { object_id },
                Node {
                    width: px(14.0),
                    height: px(14.0),
                    ..default()
                },
            ));
            row.spawn((
                Text::new(name),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(palette.text_primary),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            row.spawn((
                Node {
                    width: px(60.0),
                    height: px(8.0),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(palette.surface_vital_bg),
                BorderColor::all(palette.border_slot),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node {
                        width: percent(hp_ratio * 100.0),
                        height: percent(100.0),
                        ..default()
                    },
                    NearbyNpcHpFill { object_id },
                    BackgroundColor(hp_fill_bg),
                ));
            });
        });
}

/// Translates a click on any Nearby NPCs row into a game command. The row
/// acts as a stand-in for clicking the NPC in the world: if the player is
/// currently in spell-targeting or use-on-targeting mode, the click resolves
/// that mode against this NPC and clears the targeting state. Otherwise the
/// click sets the NPC as the player's combat target.
///
/// Runs before `CommandIntercept` *and* before `handle_use_on_targeting` /
/// `handle_spell_targeting` so those world-targeting systems see the cleared
/// state and don't double-fire from the same left-click.
#[allow(clippy::too_many_arguments)]
pub fn handle_nearby_npc_row_clicks(
    row_query: Query<(&Interaction, &NearbyNpcRow), Changed<Interaction>>,
    docked_panel_state: Res<DockedPanelState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut use_on_state: ResMut<UseOnState>,
    mut spell_targeting_state: ResMut<SpellTargetingState>,
    mut cursor_state: ResMut<CursorState>,
) {
    for (interaction, row) in row_query.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let object_id = row.object_id;

        // Active spell targeting → cast at this NPC.
        if let (Some(source), Some(spell_id)) = (
            spell_targeting_state.source,
            spell_targeting_state.spell_id.clone(),
        ) {
            if let Some(source_ref) = context_target_to_item_reference(source, &docked_panel_state)
            {
                pending_commands.push(GameCommand::CastSpellAt {
                    source: source_ref,
                    spell_id,
                    target_object_id: object_id,
                });
            }
            spell_targeting_state.source = None;
            spell_targeting_state.spell_id = None;
            cursor_state.reset_to_default();
            continue;
        }

        // Active use-on → use the source item on this NPC.
        if let Some(source) = use_on_state.source {
            if let Some(source_ref) = context_target_to_item_reference(source, &docked_panel_state)
            {
                pending_commands.push(GameCommand::UseItemOn {
                    source: source_ref,
                    target: UseTarget::Object(object_id),
                });
            }
            use_on_state.source = None;
            cursor_state.reset_to_default();
            continue;
        }

        // Default: this NPC becomes the player's combat target.
        pending_commands.push(GameCommand::SetCombatTarget {
            target_object_id: Some(object_id),
        });
    }
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
    // Iterate (rather than `.single_mut`) because the status panel can
    // exist in two places at once — the docked sidebar instance and a
    // floating MovableWindow — when `StatusPanelMode == Floating` is
    // being transitioned in/out. Each `CarryWeightLabel` gets the same
    // text.
    for (mut text, mut color) in &mut label_query {
        if text.0 != new_text {
            text.0 = new_text.clone();
        }
        if color.0 != new_color {
            color.0 = new_color;
        }
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
            crate::ui::components::HudRoot,
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
    let new_text = match &client_state.regen_buff {
        Some(buff) if buff.remaining_seconds > 0.0 => {
            let total_seconds = buff.remaining_seconds.ceil() as i32;
            let mins = total_seconds / 60;
            let secs = total_seconds % 60;
            format!("Well Fed: {mins}:{secs:02} (x{:.1})", buff.multiplier)
        }
        _ => String::new(),
    };
    // See `sync_carry_weight_label` for why we iterate.
    for mut text in &mut label_query {
        if text.0 != new_text {
            text.0 = new_text.clone();
        }
    }
}

pub fn sync_magic_effects_label(
    client_state: Res<ClientGameState>,
    mut label_query: Query<&mut Text, With<crate::ui::components::MagicEffectsLabel>>,
) {
    let new_text = if client_state.active_effects.is_empty() {
        String::new()
    } else {
        client_state
            .active_effects
            .iter()
            .map(|effect| {
                let total_seconds = effect.remaining_seconds.ceil().max(0.0) as i32;
                let mins = total_seconds / 60;
                let secs = total_seconds % 60;
                format!("{}: {mins}:{secs:02}", effect_label(effect.kind))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    // See `sync_carry_weight_label` for why we iterate.
    for mut text in &mut label_query {
        if text.0 != new_text {
            text.0 = new_text.clone();
        }
    }
}

fn effect_label(kind: crate::magic::resources::EffectKind) -> &'static str {
    use crate::magic::resources::EffectKind;
    match kind {
        EffectKind::Glimmer => "Glimmer",
        EffectKind::Haste => "Haste",
        EffectKind::Shield => "Shield",
        EffectKind::Bless => "Bless",
        EffectKind::Slow => "Slow",
        EffectKind::Sleep => "Sleep",
        EffectKind::Paralyze => "Paralyzed",
        EffectKind::Chill => "Chilled",
        EffectKind::Burning => "Burning",
        EffectKind::Poisoned => "Poisoned",
        EffectKind::Drunk => "Drunk",
    }
}

/// Mirror `ClientGameState.chat_log_lines` into the `ChatTerminal` widget.
/// `Local<Vec<String>>` tracks the last mirrored snapshot so we can detect
/// any change — appends, shrinks, or rotations once the bounded ChatLog hits
/// its `max_lines` cap (without a content-aware compare, a chat message that
/// rotates the oldest line out would slip past since the length stays the
/// same). On any mismatch we re-sync the whole buffer.
pub fn sync_chat_log(
    client_state: Res<ClientGameState>,
    mut last_lines: Local<Vec<String>>,
    mut chat_query: Query<&mut bevy_terminal::Terminal, With<ChatTerminal>>,
) {
    let Ok(mut terminal) = chat_query.single_mut() else {
        return;
    };
    if *last_lines == client_state.chat_log_lines {
        return;
    }
    // Fast path: pure append. Keeps the widget's existing scroll position
    // since `terminal.push` doesn't reset scroll. Falls through to a full
    // resync if the prefix diverges.
    if client_state.chat_log_lines.len() >= last_lines.len()
        && client_state.chat_log_lines[..last_lines.len()] == last_lines[..]
    {
        for line in &client_state.chat_log_lines[last_lines.len()..] {
            terminal.push(line.clone(), classify_chat_line(line));
        }
    } else {
        terminal.clear();
        for line in &client_state.chat_log_lines {
            terminal.push(line.clone(), classify_chat_line(line));
        }
    }
    *last_lines = client_state.chat_log_lines.clone();
}

fn classify_chat_line(line: &str) -> bevy_terminal::LineStyle {
    if line.starts_with("[System]") || line.starts_with("[System ") {
        bevy_terminal::LineStyle::ChatSystem
    } else if line.contains("whispers") {
        bevy_terminal::LineStyle::ChatWhisper
    } else {
        bevy_terminal::LineStyle::ChatSay
    }
}

pub fn sync_context_menu_root(
    context_menu_state: Res<ContextMenuState>,
    ui_scale: Res<UiScale>,
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

    // `computed_node.size()` is physical pixels; `window.width()/height()` and
    // `context_menu_state.position` are cursor-logical pixels. Bring the menu
    // size into the same space before clamping.
    let menu_size = computed_node.size() * computed_node.inverse_scale_factor();
    let max_left = (window.width() - menu_size.x).max(0.0);
    let max_top = (window.height() - menu_size.y).max(0.0);
    let clamped = Vec2::new(
        context_menu_state.position.x.clamp(0.0, max_left),
        context_menu_state.position.y.clamp(0.0, max_top),
    );
    let anchor = cursor_to_val_px(clamped, &ui_scale);
    root_node.left = px(anchor.x);
    root_node.top = px(anchor.y);
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

pub fn sync_context_menu_pick_lock_button(
    context_menu_state: Res<ContextMenuState>,
    mut button_query: Query<&mut Node, With<crate::ui::components::ContextMenuPickLockButton>>,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };
    node.display = if context_menu_state.can_pick_lock {
        Display::Flex
    } else {
        Display::None
    };
}

pub fn sync_context_menu_force_lock_button(
    context_menu_state: Res<ContextMenuState>,
    mut button_query: Query<&mut Node, With<crate::ui::components::ContextMenuForceLockButton>>,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };
    node.display = if context_menu_state.can_force_lock {
        Display::Flex
    } else {
        Display::None
    };
}

pub fn sync_context_menu_use_key_button(
    context_menu_state: Res<ContextMenuState>,
    mut button_query: Query<&mut Node, With<crate::ui::components::ContextMenuUseKeyButton>>,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };
    node.display = if context_menu_state.can_use_key {
        Display::Flex
    } else {
        Display::None
    };
}

pub fn sync_context_menu_hide_button(
    context_menu_state: Res<ContextMenuState>,
    mut button_query: Query<&mut Node, With<crate::ui::components::ContextMenuHideButton>>,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };
    node.display = if context_menu_state.can_hide {
        Display::Flex
    } else {
        Display::None
    };
}

pub fn sync_context_menu_read_button(
    context_menu_state: Res<ContextMenuState>,
    mut button_query: Query<&mut Node, With<crate::ui::components::ContextMenuReadButton>>,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };
    node.display = if context_menu_state.can_read {
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
    mut button_query: Query<&mut Node, With<crate::ui::components::ContextMenuOfferToTradeButton>>,
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
    mut pending_item_details: ResMut<crate::ui::item_details::PendingItemDetailsOpens>,
    client_state: Res<ClientGameState>,
    mut take_partial_state: ResMut<TakePartialState>,
    ui_state: (
        ResMut<ContextMenuState>,
        ResMut<DockedPanelState>,
        ResMut<CursorState>,
        ResMut<UseOnState>,
        ResMut<SpellTargetingState>,
        ResMut<ItemTargetingState>,
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
        mut item_targeting_state,
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
            match target {
                ContextMenuTarget::Slot(kind) => {
                    if let Some(slot_ref) = item_slot_kind_to_ref(kind, &docked_panel_state) {
                        pending_commands.push(GameCommand::Inspect {
                            target: InspectTarget::SlotItem(slot_ref),
                        });
                    }
                    // Trade-side slots have no `ItemSlotRef`, but the popup
                    // still works because the renderer reads from
                    // `current_trade` directly.
                    pending_item_details.slots.push(kind);
                }
                ContextMenuTarget::World(object_id) => {
                    pending_commands.push(GameCommand::Inspect {
                        target: InspectTarget::Object(object_id),
                    });
                }
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
                    match spell.targeting {
                        SpellTargeting::Targeted => {
                            spell_targeting_state.source = Some(target);
                            spell_targeting_state.spell_id = Some(spell_id);
                            cursor_state.mode = CursorMode::SpellTarget;
                            context_menu_state.hide();
                            return;
                        }
                        SpellTargeting::TargetedTile => {
                            spell_targeting_state.source = Some(target);
                            spell_targeting_state.spell_id = Some(spell_id);
                            cursor_state.mode = CursorMode::SpellTargetTile;
                            context_menu_state.hide();
                            return;
                        }
                        SpellTargeting::TargetedItem => {
                            item_targeting_state.source = Some(target);
                            item_targeting_state.spell_id = Some(spell_id);
                            cursor_state.mode = CursorMode::ItemTarget;
                            context_menu_state.hide();
                            return;
                        }
                        SpellTargeting::Untargeted => {}
                    }
                }
            }

            // Modifier-granting consumable (e.g. poison flask): like an
            // item-target spell, the player picks the item to enchant. We
            // enter the same targeting mode but with `spell_id = None`, so the
            // click dispatches `UseItemOn { target: ItemSlot }` instead.
            if target_grants_item_modifier(
                target,
                &client_state,
                &docked_panel_state,
                &object_registry,
                &definitions,
            ) {
                item_targeting_state.source = Some(target);
                item_targeting_state.spell_id = None;
                cursor_state.mode = CursorMode::ItemTarget;
                context_menu_state.hide();
                return;
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
            let usable_and_type_id: Option<String> = match target {
                ContextMenuTarget::World(object_id) => {
                    if object_is_usable(object_id, &object_registry, &definitions) {
                        object_registry.type_id(object_id).map(str::to_owned)
                    } else {
                        None
                    }
                }
                ContextMenuTarget::Slot(slot_kind) => {
                    stack_in_slot_kind(&client_state, &docked_panel_state, slot_kind).and_then(
                        |stack| {
                            definitions
                                .get(&stack.type_id)
                                .filter(|d| d.is_usable())
                                .map(|_| stack.type_id.clone())
                        },
                    )
                }
            };
            if let Some(type_id) = usable_and_type_id {
                use_on_state.source = Some(target);
                cursor_state.mode = CursorMode::UseOn;
                cursor_state.use_on_sprite = resolve_use_on_sprite(&type_id, &definitions);
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
}

/// Fallback dismiss: any LMB click that survives the button handlers above
/// closes the context menu. Scheduled `.after` the button handlers and the
/// trade/lock variants so a click on an actual menu button still runs its
/// action first (those handlers hide the menu themselves, after which this
/// is a no-op).
pub fn close_context_menu_on_lmb(
    mouse_input: Res<ButtonInput<MouseButton>>,
    mut context_menu_state: ResMut<ContextMenuState>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) || !context_menu_state.is_visible() {
        return;
    }
    context_menu_state.hide();
}

/// Handler split out from `handle_context_menu_actions` so the lock-related
/// verb buttons don't push the parent ParamSet over Bevy's 8-query cap.
pub fn handle_context_menu_lock_actions(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut context_menu_state: ResMut<ContextMenuState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut menu_queries: ParamSet<(
        Query<
            (&ComputedNode, &UiGlobalTransform),
            With<crate::ui::components::ContextMenuPickLockButton>,
        >,
        Query<
            (&ComputedNode, &UiGlobalTransform),
            With<crate::ui::components::ContextMenuForceLockButton>,
        >,
        Query<
            (&ComputedNode, &UiGlobalTransform),
            With<crate::ui::components::ContextMenuUseKeyButton>,
        >,
        Query<
            (&ComputedNode, &UiGlobalTransform),
            With<crate::ui::components::ContextMenuHideButton>,
        >,
    )>,
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

    let Some(ContextMenuTarget::World(object_id)) = context_menu_state.target else {
        return;
    };

    let verb = if context_menu_state.can_pick_lock
        && is_cursor_over_button(cursor_position, &menu_queries.p0())
    {
        Some("pick_lock")
    } else if context_menu_state.can_force_lock
        && is_cursor_over_button(cursor_position, &menu_queries.p1())
    {
        Some("force_lock")
    } else if context_menu_state.can_use_key
        && is_cursor_over_button(cursor_position, &menu_queries.p2())
    {
        Some("use_key")
    } else {
        None
    };

    if let Some(verb) = verb {
        pending_commands.push(GameCommand::InteractWithObject {
            object_id,
            verb: verb.to_owned(),
        });
        context_menu_state.hide();
        return;
    }

    if context_menu_state.can_hide && is_cursor_over_button(cursor_position, &menu_queries.p3()) {
        pending_commands.push(GameCommand::HideObject { object_id });
        context_menu_state.hide();
    }
}

/// Read-button handler. Split out from the main context-menu actions so we
/// can resolve `ItemReference` from both World and Slot targets without
/// pushing the parent ParamSet over Bevy's query cap.
pub fn handle_context_menu_read_action(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    docked_panel_state: Res<DockedPanelState>,
    mut context_menu_state: ResMut<ContextMenuState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    button_query: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<crate::ui::components::ContextMenuReadButton>,
    >,
) {
    if !mouse_input.just_pressed(MouseButton::Left) || !context_menu_state.is_visible() {
        return;
    }
    if !context_menu_state.can_read {
        return;
    }
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    if !is_cursor_over_button(cursor_position, &button_query) {
        return;
    }
    let Some(target) = context_menu_state.target else {
        return;
    };
    let Some(source) = context_target_to_item_reference(target, &docked_panel_state) else {
        return;
    };
    pending_commands.push(GameCommand::ReadBook { source });
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
            let target = if client_state
                .remote_players
                .values()
                .any(|p| p.object_id == object_id)
            {
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
        if let (Some(session_id), Some(ContextMenuTarget::Slot(slot_kind))) =
            (trade_popup_state.session_id, context_menu_state.target)
        {
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
        cursor_state.reset_to_default();
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
        cursor_state.reset_to_default();
        return;
    }

    if let Some(object) = topmost_object_at_cursor(
        &client_state,
        window,
        cursor_position,
        &player_position,
        &world_config,
        |o| is_near_player(&player_position, &o.tile_position),
    ) {
        pending_commands.push(GameCommand::UseItemOn {
            source,
            target: UseTarget::Object(object.object_id),
        });
        use_on_state.source = None;
        cursor_state.reset_to_default();
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
        cursor_state.reset_to_default();
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

    let is_tile_target = cursor_state.mode == CursorMode::SpellTargetTile;

    if is_tile_target {
        if let Some(source) = context_target_to_item_reference(source_target, &docked_panel_state) {
            pending_commands.push(GameCommand::CastSpellAtTile {
                source,
                spell_id: spell_id.to_owned(),
                target_tile,
            });
        }
    } else {
        let selected_target = topmost_object_at_cursor(
            &client_state,
            window,
            cursor_position,
            &player_position,
            &world_config,
            |o| o.is_npc,
        )
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
    }

    spell_targeting_state.source = None;
    spell_targeting_state.spell_id = None;
    cursor_state.reset_to_default();
}

/// Resolve a `CursorMode::ItemTarget` session: the next left-click on one of
/// the player's own inventory/equipment slots picks the item to enchant.
/// Dispatches `CastSpellAtItem` (item-target spell) or `UseItemOn` with
/// `UseTarget::ItemSlot` (modifier-granting consumable like a poison flask),
/// depending on whether `item_targeting_state.spell_id` is set. ESC, a
/// right-click, or a click that misses every slot cancels / is ignored.
#[allow(clippy::too_many_arguments)]
pub fn handle_item_targeting(
    mouse_input: Res<ButtonInput<MouseButton>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    state_resources: (Res<ContextMenuState>, Res<DockedPanelState>),
    mut pending_commands: ResMut<PendingGameCommands>,
    mut cursor_state: ResMut<CursorState>,
    mut item_targeting_state: ResMut<ItemTargetingState>,
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
    if cursor_state.mode != CursorMode::ItemTarget {
        return;
    }
    let (context_menu_state, docked_panel_state) = state_resources;
    let Some(source_target) = item_targeting_state.source else {
        // Mode set without a source — recover to the default cursor.
        cursor_state.reset_to_default();
        return;
    };

    if keyboard_input.just_pressed(KeyCode::Escape) || mouse_input.just_pressed(MouseButton::Right)
    {
        item_targeting_state.source = None;
        item_targeting_state.spell_id = None;
        cursor_state.reset_to_default();
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

    // A click that lands on no slot is ignored — keep targeting active so the
    // player can try again (matches "click the item you want to enchant").
    let Some(slot_kind) = hovered_slot_kind_from_ui(cursor_position, &mut slot_queries) else {
        return;
    };
    let Some(target_slot) = item_slot_kind_to_ref(slot_kind, &docked_panel_state) else {
        return;
    };
    let Some(source) = context_target_to_item_reference(source_target, &docked_panel_state) else {
        item_targeting_state.source = None;
        item_targeting_state.spell_id = None;
        cursor_state.reset_to_default();
        return;
    };

    if let Some(spell_id) = item_targeting_state.spell_id.clone() {
        pending_commands.push(GameCommand::CastSpellAtItem {
            source,
            spell_id,
            target: target_slot,
        });
    } else {
        pending_commands.push(GameCommand::UseItemOn {
            source,
            target: UseTarget::ItemSlot(target_slot),
        });
    }

    item_targeting_state.source = None;
    item_targeting_state.spell_id = None;
    cursor_state.reset_to_default();
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
        cursor_state.reset_to_default();
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

    let world_target = topmost_object_at_cursor(
        &client_state,
        window,
        cursor_position,
        &player_position,
        &world_config,
        |o| o.is_npc,
    )
    .map(|object| object.object_id);
    let remote_target = topmost_remote_player_at_cursor(
        &client_state,
        window,
        cursor_position,
        &player_position,
        &world_config,
    )
    .map(|player| player.object_id);

    let Some(target_object_id) = world_target.or(remote_target) else {
        return;
    };

    pending_commands.push(GameCommand::SetCombatTarget {
        target_object_id: Some(target_object_id),
    });

    cursor_state.reset_to_default();
}

/// Update the yellow tile highlight and the "Athletics +X vs DC Y" info box
/// while `CursorMode::JumpTarget` is active. Both UI elements stay hidden
/// outside that mode. The DC display is computed from `ClientGameState`
/// (skill ranks + replicated attributes), so embedded and remote clients see
/// the same numbers the server will roll against.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_jump_targeting_ui(
    mut cursor_state: ResMut<CursorState>,
    client_state: Res<ClientGameState>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    ui_scale: Res<UiScale>,
    definitions: Res<OverworldObjectDefinitions>,
    mut highlight_query: Query<
        (
            &mut Node,
            &mut Visibility,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (With<JumpTileHighlight>, Without<JumpInfoBoxRoot>),
    >,
    mut info_query: Query<
        (&mut Node, &mut Visibility),
        (With<JumpInfoBoxRoot>, Without<JumpTileHighlight>),
    >,
    mut info_label_query: Query<&mut Text, With<JumpInfoBoxLabel>>,
) {
    use crate::game::traversal::{
        bresenham_line, jump_cost, jump_dc, JUMP_MAX_RANGE, JUMP_MIN_RANGE,
    };
    use crate::player::classes::ability_mod;
    use crate::player::skills::Skill;

    let Ok((mut highlight_node, mut highlight_vis, mut highlight_bg, mut highlight_border)) =
        highlight_query.single_mut()
    else {
        return;
    };
    let Ok((mut info_node, mut info_vis)) = info_query.single_mut() else {
        return;
    };
    let Ok(mut info_label) = info_label_query.single_mut() else {
        return;
    };

    if cursor_state.mode != CursorMode::JumpTarget {
        *highlight_vis = Visibility::Hidden;
        *info_vis = Visibility::Hidden;
        return;
    }

    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        *highlight_vis = Visibility::Hidden;
        *info_vis = Visibility::Hidden;
        return;
    };
    let Some(player_position) = client_state.player_tile_position else {
        *highlight_vis = Visibility::Hidden;
        *info_vis = Visibility::Hidden;
        return;
    };

    // Tile-lock: the reticle latches onto a world tile when the cursor first
    // moves over it, and stays put while the character walks. That way the
    // displayed DC/range refresh as the player closes on or retreats from the
    // chosen tile, instead of being glued to the cursor's screen offset (which
    // would never change since the camera follows the player).
    let cursor_moved = cursor_state.jump_last_cursor != Some(cursor_position);
    if cursor_moved || cursor_state.jump_target_tile.is_none() {
        cursor_state.jump_target_tile = Some(cursor_to_tile(
            window,
            cursor_position,
            &player_position,
            &world_config,
        ));
        cursor_state.jump_last_cursor = Some(cursor_position);
    }
    let target_tile = cursor_state.jump_target_tile.unwrap();
    let dx = target_tile.x - player_position.x;
    let dy = target_tile.y - player_position.y;
    let xy_cost = jump_cost(dx, dy, 0);

    // Position the tile highlight at the snapped tile's screen rect. Tile
    // sprites are centered at `window_center + (tile - player) * tile_size`
    // in logical/cursor pixels; the highlight Node sits in the same pixel
    // space (cursor_to_val_px keeps it scaled correctly under UiScale).
    let tile_size = world_config.tile_size;
    let window_center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let center_logical = Vec2::new(
        window_center.x + (target_tile.x - player_position.x) as f32 * tile_size,
        window_center.y - (target_tile.y - player_position.y) as f32 * tile_size,
    );
    let top_left = center_logical - Vec2::splat(tile_size * 0.5);
    let top_left_ui = cursor_to_val_px(top_left, &ui_scale);
    let size_ui = tile_size / ui_scale.0.max(0.0001);

    highlight_node.left = px(top_left_ui.x);
    highlight_node.top = px(top_left_ui.y);
    highlight_node.width = px(size_ui);
    highlight_node.height = px(size_ui);

    let in_range = (JUMP_MIN_RANGE..=JUMP_MAX_RANGE).contains(&xy_cost);
    if in_range {
        *highlight_bg = BackgroundColor(Color::srgba(1.0, 0.92, 0.2, 0.28));
        *highlight_border = BorderColor::all(Color::srgba(1.0, 0.85, 0.1, 0.95));
    } else {
        *highlight_bg = BackgroundColor(Color::srgba(0.85, 0.25, 0.25, 0.22));
        *highlight_border = BorderColor::all(Color::srgba(0.95, 0.35, 0.35, 0.85));
    }
    *highlight_vis = Visibility::Visible;

    // Position the info box just below+right of the cursor, like
    // `sync_item_tooltip`. Keep the same offsets so both feel coherent.
    let anchor = cursor_to_val_px(cursor_position, &ui_scale);
    info_node.left = px(anchor.x + 18.0);
    info_node.top = px(anchor.y + 18.0);
    *info_vis = Visibility::Visible;

    info_label.0 = if xy_cost == 0 {
        "Pick a tile to jump to.".to_owned()
    } else if !in_range {
        format!("Out of range (cost {xy_cost}, max {JUMP_MAX_RANGE}).")
    } else {
        let ranks = client_state.skill_ranks[Skill::Athletics.index()] as i32;
        let str_mod = client_state
            .attributes
            .as_ref()
            .map(|a| ability_mod(a.strength))
            .unwrap_or(0);
        let bonus = ranks + str_mod;
        // Mirror the server's path-aware DC: walk the Bresenham line from
        // player → target, summing the highest obstacle the arc has to clear.
        // The replicated `world_objects` map gives us every projected stack;
        // floor-maps painted above are skipped — the server is authoritative
        // and the tooltip is just close-enough player feedback.
        let space_id = client_state
            .player_position
            .map(|p| p.space_id)
            .or(client_state.current_space.as_ref().map(|s| s.space_id));
        let source_z = player_position.z;
        let mut apex_dz = 0i32;
        let mut target_landing_dz = 0i32;
        if let Some(space_id) = space_id {
            let stack_top_at = |x: i32, y: i32| -> i32 {
                client_state
                    .world_objects
                    .values()
                    .filter(|obj| {
                        obj.position.space_id == space_id
                            && obj.tile_position.x == x
                            && obj.tile_position.y == y
                    })
                    .filter_map(|obj| {
                        let def = definitions.get(&obj.definition_id)?;
                        if def.render.block_size == 0 {
                            return None;
                        }
                        Some(obj.tile_position.z + def.render.block_size as i32)
                    })
                    .max()
                    .unwrap_or(0)
            };
            let walkable_top_at = |x: i32, y: i32| -> Option<i32> {
                client_state
                    .world_objects
                    .values()
                    .filter(|obj| {
                        obj.position.space_id == space_id
                            && obj.tile_position.x == x
                            && obj.tile_position.y == y
                    })
                    .filter_map(|obj| {
                        let def = definitions.get(&obj.definition_id)?;
                        if def.render.block_size == 0 || !def.render.walkable_surface {
                            return None;
                        }
                        Some(obj.tile_position.z + def.render.block_size as i32)
                    })
                    .max()
            };
            let path = bresenham_line(
                (player_position.x, player_position.y),
                (target_tile.x, target_tile.y),
            );
            for (i, (x, y)) in path.iter().copied().enumerate() {
                if i + 1 == path.len() {
                    // Target tile: prefer a walkable top here as landing;
                    // fall through to ground (z=0) when the column's top
                    // is not walkable.
                    target_landing_dz = (walkable_top_at(x, y).unwrap_or(0) - source_z).max(0);
                } else {
                    // Intermediate tile: feeds apex regardless of walkability.
                    apex_dz = apex_dz.max((stack_top_at(x, y) - source_z).max(0));
                }
            }
        }
        let max_clear = apex_dz.max(target_landing_dz);
        let dc = jump_dc(dx, dy, max_clear);
        // d20 + bonus ≥ dc → success roll range = max(1, 21 - (dc - bonus)).
        let need = (dc - bonus).clamp(1, 20);
        let success_pct = ((21 - need).max(0) * 5).min(100);
        format!(
            "Athletics {sign}{bonus} vs DC {dc}  (~{success_pct}%)",
            sign = if bonus >= 0 { "+" } else { "" }
        )
    };
}

pub fn handle_jump_targeting(
    mouse_input: Res<ButtonInput<MouseButton>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    context_menu_state: Res<ContextMenuState>,
    client_state: Res<ClientGameState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut cursor_state: ResMut<CursorState>,
) {
    use crate::game::traversal::{jump_cost, JUMP_MAX_RANGE, JUMP_MIN_RANGE};

    if cursor_state.mode != CursorMode::JumpTarget {
        return;
    }

    if keyboard_input.just_pressed(KeyCode::Escape) || mouse_input.just_pressed(MouseButton::Right)
    {
        cursor_state.reset_to_default();
        return;
    }

    if context_menu_state.is_visible() || !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }

    // Prefer the tile-locked target (synced by `sync_jump_targeting_ui`) so
    // the click resolves to whatever world tile the reticle is sitting on,
    // not the cursor's instantaneous screen position. The fallback handles
    // the rare frame where the sync system hasn't run yet (initial entry).
    let target_tile = if let Some(t) = cursor_state.jump_target_tile {
        t
    } else {
        let Ok(window) = window_query.single() else {
            return;
        };
        let Some(cursor_position) = window.cursor_position() else {
            return;
        };
        let Some(player_position) = client_state.player_tile_position else {
            return;
        };
        cursor_to_tile(window, cursor_position, &player_position, &world_config)
    };
    let Some(player_position) = client_state.player_tile_position else {
        return;
    };
    let dx = target_tile.x - player_position.x;
    let dy = target_tile.y - player_position.y;
    let xy_cost = jump_cost(dx, dy, 0);
    if !(JUMP_MIN_RANGE..=JUMP_MAX_RANGE).contains(&xy_cost) {
        // Out-of-range click stays in jump mode so the player can retarget.
        return;
    }

    pending_commands.push(GameCommand::JumpTo { target_tile });
    cursor_state.reset_to_default();
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
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
    mut item_targeting_state: ResMut<ItemTargetingState>,
    mut cursor_state: ResMut<CursorState>,
    nearby_npc_rows: Query<(&NearbyNpcRow, &ComputedNode, &UiGlobalTransform)>,
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
        cursor_state.reset_to_default();
        context_menu_state.hide();
        return;
    }

    if use_on_state.source.is_some() {
        use_on_state.source = None;
        cursor_state.reset_to_default();
        context_menu_state.hide();
        return;
    }

    if item_targeting_state.source.is_some() {
        item_targeting_state.source = None;
        item_targeting_state.spell_id = None;
        cursor_state.reset_to_default();
        context_menu_state.hide();
        return;
    }

    // Right-clicking on a Nearby NPCs row treats that NPC as the right-click
    // target — same context menu flags as right-clicking the NPC in the world.
    if let Some(npc_object_id) = nearby_npc_rows
        .iter()
        .find(|(_, computed, transform)| point_in_ui_node(cursor_position, computed, transform))
        .map(|(row, _, _)| row.object_id)
    {
        if let Some(object) = client_state.world_objects.get(&npc_object_id) {
            let near = is_near_player(&player_position, &object.tile_position);
            let can_use =
                near && object_is_usable(object.object_id, &object_registry, &definitions);
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
                near && object.is_shopkeeper,
                interaction,
            );
            if near {
                let (pick, force, key) = lock_verb_visibility(object, &definitions, &client_state);
                context_menu_state.set_lock_verbs(pick, force, key);
                context_menu_state.set_can_hide(hide_verb_visibility(
                    object,
                    &definitions,
                    &client_state,
                ));
                context_menu_state
                    .set_can_read(can_read_target(&object.definition_id, &definitions));
            }
            return;
        }
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
            context_menu_state.set_can_read(can_read_target(&stack.type_id, &definitions));
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
    if let Some(remote_player) = topmost_remote_player_at_cursor(
        &client_state,
        window,
        cursor_position,
        &player_position,
        &world_config,
    ) {
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

    // Priority: any NPC at the cursor wins (so an NPC standing on top of a
    // pickup is the right-click target, not the pickup); otherwise the
    // topmost object in the column.
    let best_object = topmost_object_at_cursor(
        &client_state,
        window,
        cursor_position,
        &player_position,
        &world_config,
        |o| o.is_npc,
    )
    .or_else(|| {
        topmost_object_at_cursor(
            &client_state,
            window,
            cursor_position,
            &player_position,
            &world_config,
            |_| true,
        )
    });

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
        if near {
            let (pick, force, key) = lock_verb_visibility(object, &definitions, &client_state);
            context_menu_state.set_lock_verbs(pick, force, key);
            context_menu_state.set_can_hide(hide_verb_visibility(
                object,
                &definitions,
                &client_state,
            ));
            context_menu_state.set_can_read(can_read_target(&object.definition_id, &definitions));
        }
        info!(
            "context_open_world_success object_id={} has_container={} can_use={} can_attack={} near={}",
            object.object_id, object.is_container, can_use, object.is_npc, near
        );
        return;
    }

    info!("context_open_no_target");
    context_menu_state.hide();
}

/// True when `type_id`'s definition declares `text_kind` (book / tombstone)
/// or `engravable: true`. Drives the "Read" context-menu verb visibility for
/// both world objects and inventory slots.
fn can_read_target(type_id: &str, definitions: &OverworldObjectDefinitions) -> bool {
    definitions
        .get(type_id)
        .is_some_and(|def| def.text_kind.is_some() || def.engravable)
}

pub fn sync_docked_panel_layout(
    docked_panel_state: Res<DockedPanelState>,
    mut panel_queries: ParamSet<(
        Query<(&DockedPanelRoot, &mut Node, &mut Visibility), With<DockedPanelRoot>>,
        Query<(&DockedPanelCloseButton, &mut Visibility), With<DockedPanelCloseButton>>,
        Query<(&DockedPanelResizeHandle, &mut Visibility), With<DockedPanelResizeHandle>>,
    )>,
) {
    // Panels rendered as a floating window are still in
    // `docked_panel_state.panels` so slot resolution keeps working;
    // we just skip their sidebar row here. The floating set is
    // maintained by `sync_panel_floating_lifecycle` across every
    // `MountablePanel` impl.
    let is_floating = |panel_id: usize| docked_panel_state.is_floating(panel_id);

    for (panel_root, mut node, mut visibility) in &mut panel_queries.p0() {
        let panel = docked_panel_state.panel(panel_root.panel_id);
        let floating = is_floating(panel_root.panel_id);
        if let Some(panel) = panel.filter(|_| !floating) {
            let top_offset = docked_panel_state
                .panels
                .iter()
                .take_while(|candidate| candidate.id != panel_root.panel_id)
                .filter(|candidate| !is_floating(candidate.id))
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
            && !is_floating(close_button.panel_id)
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
            && !is_floating(resize_handle.panel_id)
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
            Some(DockedPanelKind::NearbyNpcs) => "Nearby NPCs".to_owned(),
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

        let state = stack.properties.get("state").map(String::as_str);
        let Some(sprite_path) = definition
            .sprite_path_for_state_count(state, stack.quantity)
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
    mut label_query: Query<(&EquipmentSlotLabel, &mut Visibility), Without<EquipmentSlotImage>>,
) {
    for (slot, mut image_node, mut visibility) in &mut image_query {
        let equipment_slot = match slot.kind {
            ItemSlotKind::Equipment(s) => Some(s),
            _ => None,
        };
        let item = equipment_slot.and_then(|s| client_state.inventory.equipment_item(s).cloned());

        let sprite_path = item.as_ref().and_then(|item| {
            let definition = definitions.get(&item.type_id)?;
            let state = item.properties.get("state").map(String::as_str);
            definition.sprite_path_for_state(state).map(str::to_owned)
        });

        if let Some(sprite_path) = sprite_path {
            image_node.image = asset_server.load(sprite_path);
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }

        if let Some(equipment_slot) = equipment_slot {
            let occupied = matches!(*visibility, Visibility::Visible);
            for (label, mut label_vis) in &mut label_query {
                if label.slot == equipment_slot {
                    *label_vis = if occupied {
                        Visibility::Hidden
                    } else {
                        Visibility::Inherited
                    };
                }
            }
        }
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
    interaction_state: (
        Res<ContextMenuState>,
        Res<UseOnState>,
        Res<ItemTargetingState>,
    ),
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    trade_popup_state: Res<crate::ui::resources::TradePopupState>,
    mut drag_state: ResMut<DragState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut quickbar: ResMut<Quickbar>,
    quickbar_slot_query: Query<
        (
            &QuickbarSlotMarker,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&Visibility>,
        ),
        With<Button>,
    >,
    object_registry: Res<ObjectRegistry>,
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
    let (context_menu_state, use_on_state, item_targeting_state) = interaction_state;
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

    // While the player is picking an item to enchant, a left-click on a slot
    // is a target selection (handled by `handle_item_targeting`), not a drag.
    if item_targeting_state.source.is_some() {
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

        if let Some(object) = topmost_object_at_cursor(
            &client_state,
            window,
            cursor_position,
            &player_position,
            &world_config,
            |o| o.is_movable && is_near_player(&player_position, &o.tile_position),
        ) {
            info!(
                "drag_start world_object_id={} origin=({}, {}, {})",
                object.object_id,
                object.tile_position.x,
                object.tile_position.y,
                object.tile_position.z,
            );
            drag_state.source = Some(DragSource::World);
            drag_state.object_id = Some(object.object_id);
            drag_state.world_origin = Some(object.tile_position);
        }
    }

    if !mouse_input.just_released(MouseButton::Left) || drag_state.source.is_none() {
        return;
    }

    // Drop on the visual ground tile under the cursor. Projecting at z=0
    // reverses the half-floor perspective shift the ground gets when the
    // player stands on a half-block, so the user can "aim down" to ground
    // level without their target jumping one tile. The server still snaps
    // the final z to the column's stack top, so dropping onto a chest's
    // visible position lands on top of the chest.
    let target_tile =
        cursor_to_ground_tile(window, cursor_position, &player_position, &world_config);
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

    // Quickbar drop has priority over the inventory/world dispatch when the
    // cursor is released over a quickbar slot. Always consume the drop so we
    // don't accidentally fall through into "drop on floor below the bar".
    if let Some(quickbar_index) = hovered_quickbar_slot(cursor_position, &quickbar_slot_query) {
        let bound_type_id: Option<String> = match &drag_source {
            Some(DragSource::UiSlot(slot_kind)) => match slot_kind {
                ItemSlotKind::Backpack(_) | ItemSlotKind::Equipment(_) => {
                    stack_in_slot_kind(&client_state, &docked_panel_state, *slot_kind)
                        .map(|stack| stack.type_id)
                }
                _ => None,
            },
            Some(DragSource::World) => {
                dragged_object_id.and_then(|id| object_registry.type_id(id).map(str::to_owned))
            }
            None => None,
        };
        if let Some(type_id) = bound_type_id {
            quickbar.assign(quickbar_index, type_id);
        }
        return;
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
                    if let Some(slot_ref) = item_slot_kind_to_ref(source_slot, &docked_panel_state)
                    {
                        let qty =
                            stack_in_slot_kind(&client_state, &docked_panel_state, source_slot)
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
    ui_scale: Res<UiScale>,
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
                let state = stack.properties.get("state").map(String::as_str);
                let sprite_path = definitions
                    .get(&stack.type_id)
                    .and_then(|def| def.sprite_path_for_state_count(state, stack.quantity))
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
    let anchor = cursor_to_val_px(cursor_position, &ui_scale);
    preview_node.left = px(anchor.x + 14.0);
    preview_node.top = px(anchor.y + 14.0);
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
    ui_scale: Res<UiScale>,
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
    let anchor = cursor_to_val_px(cursor_position, &ui_scale);
    tooltip_node.left = px(anchor.x + 18.0);
    tooltip_node.top = px(anchor.y - 24.0);
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
            drag_state.press_origin = None;
            drag_state.passed_threshold = false;
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
            drag_state.press_origin = Some(cursor_position);
            drag_state.passed_threshold = false;
            break;
        }
    }

    let Some(active_panel_id) = drag_state.panel_id else {
        return;
    };

    if mouse_input.just_released(MouseButton::Left) {
        drag_state.panel_id = None;
        drag_state.press_origin = None;
        drag_state.passed_threshold = false;
        return;
    }

    if !mouse_input.pressed(MouseButton::Left) {
        return;
    }

    // Don't start reordering until the cursor has moved past the drag
    // threshold from the click-down point — otherwise a plain click
    // on the title bar snaps the panel to wherever the cursor happens
    // to be relative to the other panel centers.
    if !drag_state.passed_threshold {
        let Some(origin) = drag_state.press_origin else {
            return;
        };
        if cursor_position.distance(origin) < crate::ui::resources::DOCKED_PANEL_DRAG_THRESHOLD_PX {
            return;
        }
        drag_state.passed_threshold = true;
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

fn hovered_quickbar_slot(
    cursor_position: Vec2,
    slot_query: &Query<
        (
            &QuickbarSlotMarker,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&Visibility>,
        ),
        With<Button>,
    >,
) -> Option<usize> {
    slot_query
        .iter()
        .find_map(|(slot, computed_node, global_transform, visibility)| {
            if visibility.is_some_and(|visibility| *visibility == Visibility::Hidden) {
                return None;
            }
            point_in_ui_node(cursor_position, computed_node, global_transform).then_some(slot.0)
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
    // `Window::cursor_position()` is in logical pixels, but `ComputedNode`
    // geometry and `UiGlobalTransform` are in physical pixels. They diverge
    // on HiDPI displays (e.g. macOS Retina, scale_factor = 2.0), so scale
    // the cursor up before hit-testing or every UI click misses.
    let inv = computed_node.inverse_scale_factor();
    let physical_cursor = if inv > 0.0 {
        cursor_position / inv
    } else {
        cursor_position
    };
    computed_node.contains_point(*global_transform, physical_cursor)
}

/// Convert a logical cursor position (from `Window::cursor_position()`) into a
/// scalar suitable for `Node.left` / `Node.top`. Bevy multiplies `Val::Px(x)`
/// by `UiScale` during layout, but the cursor is unaffected by `UiScale`, so
/// `Val::Px(cursor.x)` lands the node at `cursor.x * UiScale` in screen-space.
/// Dividing here cancels that. Mirror of the hit-test conversion in
/// `point_in_ui_node`.
pub(crate) fn cursor_to_val_px(cursor: Vec2, ui_scale: &UiScale) -> Vec2 {
    if ui_scale.0 > 0.0 {
        cursor / ui_scale.0
    } else {
        cursor
    }
}

pub(crate) fn stack_in_slot_kind(
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
    // Skip lock-gated verbs here so they don't shadow non-lock interactions
    // (e.g. "Open" from a locked door's closed state). The lock buttons are
    // surfaced via `lock_verb_visibility` instead. A "lock verb" is one
    // whose gate reads from the object's `lock` block — gather/skill-checked
    // interactions with a fixed DC (e.g. fishing's `Survival` check) are NOT
    // lock verbs and should appear on the regular Interact button.
    let interaction = definition.interactions.iter().find(|i| {
        let state_matches =
            i.from.is_empty() || current_state.is_some_and(|cs| i.from.iter().any(|s| s == cs));
        let is_lock_verb = i.key_gate.is_some()
            || matches!(
                i.skill_gate.as_ref().map(|g| g.dc),
                Some(crate::world::object_definitions::DcSource::FromLockPick)
                    | Some(crate::world::object_definitions::DcSource::FromLockForce)
            );
        // Tool-gated interactions (gathering nodes) are invoked via the
        // player's item context menu — right-click the pickaxe, then "Use On"
        // the ore node. Hide the verb here so the node's context menu doesn't
        // also offer a "Mine" button that would require an equipped tool.
        let is_tool_gated = i.tool_gate.is_some();
        state_matches && !is_lock_verb && !is_tool_gated
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

/// Returns `(can_pick_lock, can_force_lock, can_use_key)` for the hovered
/// object. Buttons appear whenever the target offers the corresponding
/// interaction from its current state — even at 0 ranks, so the player sees
/// the option and learns that ranks are needed via the server-side failure
/// chat line. `can_use_key` still requires a matching key in inventory (no
/// point surfacing a button that the server will immediately reject).
fn lock_verb_visibility(
    object: &crate::game::resources::ClientWorldObjectState,
    definitions: &OverworldObjectDefinitions,
    client_state: &ClientGameState,
) -> (bool, bool, bool) {
    let Some(definition) = definitions.get(&object.definition_id) else {
        return (false, false, false);
    };
    let current_state = object.state.as_deref();

    let state_matches = |interaction: &crate::world::object_definitions::ObjectInteractionDef| {
        interaction.from.is_empty()
            || current_state.is_some_and(|cs| interaction.from.iter().any(|s| s == cs))
    };

    let mut can_pick_lock = false;
    let mut can_force_lock = false;
    let mut can_use_key = false;
    for interaction in &definition.interactions {
        if !state_matches(interaction) {
            continue;
        }
        if let Some(skill_gate) = &interaction.skill_gate {
            match skill_gate.skill {
                crate::player::skills::Skill::Thievery => can_pick_lock = true,
                crate::player::skills::Skill::Athletics => can_force_lock = true,
                _ => {}
            }
        }
        if let Some(key_gate) = &interaction.key_gate {
            let required_id = match key_gate.source {
                crate::world::object_definitions::KeyIdSource::FromLock => {
                    definition.lock.as_ref().map(|l| l.lock_id)
                }
                crate::world::object_definitions::KeyIdSource::Fixed(id) => Some(id),
            };
            if required_id.is_some_and(|id| client_inventory_has_key(client_state, definitions, id))
            {
                can_use_key = true;
            }
        }
    }
    (can_pick_lock, can_force_lock, can_use_key)
}

/// Whether the right-click "Hide" entry should appear. Mirrors the
/// server-side gates in `world::hide_action::process_hide_commands`: the
/// object's definition must declare `can_hide:`, the object must not already
/// be hidden, and the actor needs at least 1 rank of Thievery.
fn hide_verb_visibility(
    object: &crate::game::resources::ClientWorldObjectState,
    definitions: &OverworldObjectDefinitions,
    client_state: &ClientGameState,
) -> bool {
    let Some(definition) = definitions.get(&object.definition_id) else {
        return false;
    };
    if definition.can_hide.is_none() {
        return false;
    }
    if object.is_hidden {
        return false;
    }
    client_state.skill_ranks[crate::player::skills::Skill::Thievery.index()] >= 1
}

/// Walk the projected local inventory for an item whose definition has a
/// matching `lock_id`. The server-side equivalent lives in
/// `world::interactions::inventory_has_key`; both must agree on which slots
/// to inspect so the verb visibility matches the server's apply-time check.
fn client_inventory_has_key(
    client_state: &ClientGameState,
    definitions: &OverworldObjectDefinitions,
    lock_id: u32,
) -> bool {
    for stack in client_state.inventory.backpack_slots.iter().flatten() {
        if definitions
            .get(&stack.type_id)
            .and_then(|d| d.lock_id)
            .is_some_and(|id| id == lock_id)
        {
            return true;
        }
    }
    for (_, item) in &client_state.inventory.equipment_slots {
        let Some(item) = item else {
            continue;
        };
        if definitions
            .get(&item.type_id)
            .and_then(|d| d.lock_id)
            .is_some_and(|id| id == lock_id)
        {
            return true;
        }
    }
    false
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
    _spell_definitions: &SpellDefinitions,
) -> bool {
    let Some(type_id) = object_registry.type_id(object_id) else {
        return false;
    };
    let Some(definition) = definitions.get(type_id) else {
        return false;
    };

    // Any usable item can target — gathering tools, untargeted spell scrolls,
    // healing items used on other players, etc. The server resolves what the
    // action actually does; the menu just opens the picker. Items carrying a
    // *targeted* spell route through `CursorMode::SpellTarget` separately at
    // click time (see `handle_context_menu_clicks`).
    definition.is_usable()
}

/// True when the context-menu target item's definition grants an item modifier
/// on use (e.g. a poison flask). Tells the "Use" handler to enter item-target
/// mode (so the player picks the item to enchant) instead of dispatching
/// `UseItem` immediately.
fn target_grants_item_modifier(
    target: ContextMenuTarget,
    client_state: &ClientGameState,
    docked_panel_state: &DockedPanelState,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    let type_id = match target {
        ContextMenuTarget::World(object_id) => {
            object_registry.type_id(object_id).map(str::to_owned)
        }
        ContextMenuTarget::Slot(slot_kind) => {
            stack_in_slot_kind(client_state, docked_panel_state, slot_kind)
                .map(|stack| stack.type_id.clone())
        }
    };
    type_id
        .and_then(|t| definitions.get(&t))
        .is_some_and(|d| d.use_effects.grants_item_modifier.is_some())
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

/// Maps a screen-space cursor to a tile `(x, y, z)`. The returned `z` is
/// `player.z` and the `(x, y)` is computed as if the target tile sat at the
/// player's `z` plane (no perspective correction). Useful for "is the cursor
/// on the player's own tile?" comparisons. For finding objects, use
/// [`topmost_object_at_cursor`] (per-object projection). For *placement on
/// the ground*, use [`cursor_to_ground_tile`] — when the player is on a
/// half-block, the unprojected cursor maps to the wrong tile_y for ground
/// targets because ground sprites are diagonally shifted by
/// `floor_screen_offset`.
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

/// Maps the cursor to a tile at `z = 0` (ground), reversing the ground
/// floor's perspective shift relative to the player's `z`. Use this when the
/// user is targeting *where on the ground* their action lands — drag-release
/// placement, ground-targeted spells — so the cursor stays aligned with the
/// visual ground tile under it when the player stands on a half-block.
/// Server-side resolution (`resolve_world_drop_tile` etc.) still snaps the
/// final `z` to the column's stack top, so dropping on a chest's visible
/// position still places on top of the chest.
fn cursor_to_ground_tile(
    window: &Window,
    cursor_position: Vec2,
    player_position: &TilePosition,
    world_config: &WorldConfig,
) -> TilePosition {
    let window_center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let cursor_offset = cursor_position - window_center;
    let floor_offset = crate::world::systems::floor_screen_offset(
        0.0,
        player_position.z as f32,
        world_config.tile_size,
    );
    let tile_offset_x =
        ((cursor_offset.x - floor_offset.x) / world_config.tile_size).round() as i32;
    let tile_offset_y =
        ((-cursor_offset.y - floor_offset.y) / world_config.tile_size).round() as i32;

    TilePosition::new(
        player_position.x + tile_offset_x,
        player_position.y + tile_offset_y,
        0,
    )
}

/// True iff the cursor lies within the screen-rendered footprint of a tile at
/// world coordinates `(tile_x, tile_y, tile_z)`. Camera is anchored on
/// `player_position`, and tiles at a non-player `z` are diagonally shifted by
/// `floor_screen_offset(tile_z, player.z)` — this function reverses that
/// shift per candidate so an object on the ground (`z=0`) is still clickable
/// when the player is standing on a chest (`z=1`).
fn cursor_hits_tile(
    window: &Window,
    cursor_position: Vec2,
    player_position: &TilePosition,
    world_config: &WorldConfig,
    tile_x: i32,
    tile_y: i32,
    tile_z: i32,
) -> bool {
    let window_center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let cursor_offset = cursor_position - window_center;
    // Cursor in world-space relative to camera (which sits on player tile).
    let cursor_dx = cursor_offset.x;
    let cursor_dy = -cursor_offset.y;
    let floor_offset = crate::world::systems::floor_screen_offset(
        tile_z as f32,
        player_position.z as f32,
        world_config.tile_size,
    );
    let tile_dx = (tile_x - player_position.x) as f32 * world_config.tile_size + floor_offset.x;
    let tile_dy = (tile_y - player_position.y) as f32 * world_config.tile_size + floor_offset.y;
    let half = world_config.tile_size * 0.5;
    (cursor_dx - tile_dx).abs() <= half && (cursor_dy - tile_dy).abs() <= half
}

/// Find the topmost world object whose rendered tile is under the cursor and
/// passes `predicate`. Per-object perspective projection (`cursor_hits_tile`)
/// means objects at any `z` are picked correctly — including a ground chest
/// while the player stands on another chest. Ties broken by `(z, placement_seq)`:
/// higher `z` wins, and within the same `z` the most-recently-placed item
/// wins (LIFO). Without the `placement_seq` tiebreaker, `block_size == 0`
/// items on the same tile would all share `z = 0` and pickup would resolve
/// to whichever the HashMap iterator yielded first — non-deterministic and
/// not necessarily matching the visual top.
fn topmost_object_at_cursor<'a, F>(
    client_state: &'a ClientGameState,
    window: &Window,
    cursor_position: Vec2,
    player_position: &TilePosition,
    world_config: &WorldConfig,
    mut predicate: F,
) -> Option<&'a crate::game::resources::ClientWorldObjectState>
where
    F: FnMut(&crate::game::resources::ClientWorldObjectState) -> bool,
{
    let mut best: Option<&crate::game::resources::ClientWorldObjectState> = None;
    for object in client_state.world_objects.values() {
        if !predicate(object) {
            continue;
        }
        if !cursor_hits_tile(
            window,
            cursor_position,
            player_position,
            world_config,
            object.tile_position.x,
            object.tile_position.y,
            object.tile_position.z,
        ) {
            continue;
        }
        let key = (object.tile_position.z, object.placement_seq);
        if best
            .map(|b| key > (b.tile_position.z, b.placement_seq))
            .unwrap_or(true)
        {
            best = Some(object);
        }
    }
    best
}

/// Remote-player counterpart of [`topmost_object_at_cursor`].
fn topmost_remote_player_at_cursor<'a>(
    client_state: &'a ClientGameState,
    window: &Window,
    cursor_position: Vec2,
    player_position: &TilePosition,
    world_config: &WorldConfig,
) -> Option<&'a crate::game::resources::ClientRemotePlayerState> {
    let mut best: Option<&crate::game::resources::ClientRemotePlayerState> = None;
    for player in client_state.remote_players.values() {
        if !cursor_hits_tile(
            window,
            cursor_position,
            player_position,
            world_config,
            player.tile_position.x,
            player.tile_position.y,
            player.tile_position.z,
        ) {
            continue;
        }
        if best
            .map(|b| player.tile_position.z > b.tile_position.z)
            .unwrap_or(true)
        {
            best = Some(player);
        }
    }
    best
}

/// Caches the tile under the mouse cursor into `HoveredTile` while the
/// coordinate readout is enabled. Skips the work entirely when
/// `ShowCoordinates` is off so we don't pay for it during normal play.
pub fn update_hovered_tile(
    show_coords: Res<ShowCoordinates>,
    mut hovered: ResMut<HoveredTile>,
    window_q: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    client_state: Res<crate::game::resources::ClientGameState>,
) {
    if !show_coords.0 {
        if hovered.0.is_some() {
            hovered.0 = None;
        }
        return;
    }
    let Ok(window) = window_q.single() else {
        hovered.0 = None;
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        hovered.0 = None;
        return;
    };
    let Some(player) = client_state.player_tile_position else {
        hovered.0 = None;
        return;
    };
    let tile = cursor_to_ground_tile(window, cursor, &player, &world_config);
    hovered.0 = Some(tile);
}
