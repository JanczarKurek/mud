//! Quick-use bar — a fixed bottom-center hotbar of 10 slots bound by item
//! `type_id`. Number keys `1`..`9` and `0` map to slots `0`..`9`. Plain digit
//! issues a `UseItem`; Ctrl+digit enters the existing `UseOn` cursor mode.
//!
//! Slot assignment is drag-and-drop from the backpack or equipment panel
//! (see the dispatch in `crate::ui::systems::handle_movable_dragging`). The
//! resource is purely client-side; the server never sees it, and assignments
//! persist to a JSON file under the per-role data tree (see
//! `crate::app::paths::quickbar_path`).

use std::fs;
use std::path::PathBuf;

use bevy::ecs::query::QueryFilter;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use serde::{Deserialize, Serialize};

use crate::app::plugin::AppRuntime;
use crate::game::commands::{GameCommand, ItemReference, ItemSlotRef};
use crate::game::resources::{ClientGameState, PendingGameCommands};
use crate::magic::resources::{SpellDefinitions, SpellTargeting};
use crate::player::components::{InventoryStack, PlayerId};
use crate::scripting::resources::PythonConsoleState;
use crate::ui::components::{
    BottomPanelHideButton, ChatAreaContainer, ChatPanel, ItemSlotKind, PythonConsolePanel,
    QuickbarRoot, QuickbarSlotChargesLabel, QuickbarSlotIcon, QuickbarSlotMarker,
};
use crate::ui::resources::{
    BottomPanelVisibility, ContextMenuState, ContextMenuTarget, CursorMode, CursorState, Quickbar,
    SpellTargetingState, UseOnState, QUICKBAR_SLOT_COUNT,
};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton};
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;

const SLOT_SIZE_PX: f32 = 44.0;
const ICON_SIZE_PX: f32 = 32.0;
/// Tint applied when the slot's bound type_id is no longer in inventory.
/// Alpha is high enough that the icon stays clearly recognizable while
/// still visually distinct from a held item.
const DIMMED_TINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.55);

/// On-disk format. Versioned via `serde(default)` on `slots` so older files
/// missing or extra entries don't break.
#[derive(Deserialize, Serialize, Default)]
struct QuickbarFile {
    #[serde(default)]
    slots: Vec<Option<String>>,
}

/// Tracks the last `PlayerId` we attempted to load slots for, so the load
/// runs exactly once per character login.
#[derive(Resource, Default)]
pub struct QuickbarLoadedFor {
    pub player_id: Option<PlayerId>,
}

/// Spawn the quickbar as a child of `parent`. The caller controls position;
/// `spawn_hud` parents it to the `BottomHudColumn` so the bar slides down
/// with the chat area when it's hidden.
pub fn spawn_quickbar(parent: &mut ChildSpawnerCommands, theme: &UiThemeAssets, palette: &Palette) {
    let (slot_bg, slot_border, _) = idle_colors(palette, ButtonStyle::Slot, false);

    parent
        .spawn((
            Node {
                align_self: AlignSelf::FlexStart,
                ..default()
            },
            QuickbarRoot,
            BackgroundColor(Color::NONE),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    padding: UiRect::axes(px(6.0), px(4.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                ImageNode::new(theme.panel_frame.clone())
                    .with_mode(theme.panel_image_mode())
                    .with_color(Color::WHITE),
                BackgroundColor(Color::NONE),
                BorderColor::all(palette.border_slot),
            ))
            .with_children(|bar| {
                for index in 0..QUICKBAR_SLOT_COUNT {
                    spawn_quickbar_slot(bar, theme, palette, index, slot_bg, slot_border);
                }
            });
        });
}

fn spawn_quickbar_slot(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    index: usize,
    slot_bg: Color,
    slot_border: Color,
) {
    let key_label = digit_label_for_slot(index);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Slot),
            QuickbarSlotMarker(index),
            Node {
                width: px(SLOT_SIZE_PX),
                height: px(SLOT_SIZE_PX),
                border: UiRect::all(px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                position_type: PositionType::Relative,
                ..default()
            },
            ImageNode::new(theme.slot_frame.clone())
                .with_mode(theme.slot_image_mode())
                .with_color(slot_bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(slot_border),
        ))
        .with_children(|slot| {
            slot.spawn((
                Node {
                    width: px(ICON_SIZE_PX),
                    height: px(ICON_SIZE_PX),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                ImageNode::default().with_color(Color::WHITE),
                QuickbarSlotIcon(index),
                Visibility::Hidden,
            ));
            slot.spawn((
                Text::new(key_label),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(palette.text_label_slot),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(1.0),
                    left: px(3.0),
                    ..default()
                },
            ));
            slot.spawn((
                Text::new(""),
                QuickbarSlotChargesLabel(index),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(palette.text_quantity),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: px(1.0),
                    right: px(3.0),
                    ..default()
                },
            ));
        });
}

fn digit_label_for_slot(index: usize) -> String {
    match index {
        0..=8 => (index + 1).to_string(),
        9 => "0".to_owned(),
        _ => String::new(),
    }
}

/// Repaint slot icons and charge counters from inventory.
pub fn sync_quickbar_visuals(
    quickbar: Res<Quickbar>,
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    asset_server: Res<AssetServer>,
    mut icon_query: Query<
        (&QuickbarSlotIcon, &mut ImageNode, &mut Visibility),
        Without<QuickbarSlotChargesLabel>,
    >,
    mut label_query: Query<(&QuickbarSlotChargesLabel, &mut Text), Without<QuickbarSlotIcon>>,
) {
    for (icon, mut image, mut visibility) in &mut icon_query {
        let Some(type_id) = quickbar.slots.get(icon.0).and_then(|s| s.as_deref()) else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(definition) = definitions.get(type_id) else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let Some(sprite_path) = definition.sprite_for_count(1).map(str::to_owned) else {
            *visibility = Visibility::Hidden;
            continue;
        };

        let stack = find_stack_by_type(&client_state, type_id);
        let tint = if stack.is_some() {
            Color::WHITE
        } else {
            DIMMED_TINT
        };
        image.image = asset_server.load(sprite_path);
        image.color = tint;
        *visibility = Visibility::Visible;
    }

    for (label, mut text) in &mut label_query {
        let Some(type_id) = quickbar.slots.get(label.0).and_then(|s| s.as_deref()) else {
            text.0.clear();
            continue;
        };
        let stack = find_stack_by_type(&client_state, type_id);
        let charge_or_qty = stack.as_ref().and_then(|s| {
            // Charged item -> show remaining charges. Else show stack quantity
            // when >1 (so a single potion shows nothing; a stack of 5 shows "5").
            if let Some(charges) = s.charges_remaining() {
                Some(charges.to_string())
            } else if s.quantity > 1 {
                Some(s.quantity.to_string())
            } else {
                None
            }
        });
        text.0 = charge_or_qty.unwrap_or_default();
    }
}

/// Translate digit-key presses into the appropriate use flow:
/// - bound item with a **targeted** spell (wand, offensive scroll) → enter
///   `CursorMode::SpellTarget` so the next click fires `CastSpellAt`;
/// - else plain digit → `UseItem` (self-cast healing, food, untargeted
///   spells);
/// - else `Ctrl`+digit → enter `CursorMode::UseOn` for the existing
///   tool-on-target flow.
///
/// Suppressed while:
/// - the Python console is open (it eats text input),
/// - a `UseOn` or `SpellTarget` cursor mode is already active,
/// - the context menu is open.
#[allow(clippy::too_many_arguments)]
pub fn handle_quickbar_keybinds(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    keybindings: Res<crate::ui::settings::Keybindings>,
    console_state: Option<Res<PythonConsoleState>>,
    context_menu_state: Res<ContextMenuState>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    quickbar: Res<Quickbar>,
    client_state: Res<ClientGameState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut cursor_state: ResMut<CursorState>,
    mut use_on_state: ResMut<UseOnState>,
    mut spell_targeting_state: ResMut<SpellTargetingState>,
) {
    if console_state.as_ref().is_some_and(|state| state.is_open) {
        return;
    }
    if context_menu_state.is_visible() {
        return;
    }
    if use_on_state.source.is_some() || spell_targeting_state.source.is_some() {
        return;
    }

    let ctrl_held = keyboard_input.pressed(KeyCode::ControlLeft)
        || keyboard_input.pressed(KeyCode::ControlRight);

    for slot_index in 0..QUICKBAR_SLOT_COUNT {
        let Some(key) = keybindings.quickbar_key(slot_index as u8) else {
            continue;
        };
        if !keyboard_input.just_pressed(key) {
            continue;
        }
        let Some(type_id) = quickbar.slots.get(slot_index).and_then(|s| s.as_deref()) else {
            continue;
        };
        let Some(slot_kind) = find_slot_kind_for_type(&client_state, type_id) else {
            continue;
        };
        let slot_ref = item_slot_kind_to_inventory_ref(slot_kind);
        let target = ContextMenuTarget::Slot(slot_kind);

        // Targeted-spell items (wands, offensive scrolls) need a target picked
        // before the cast — mirror the context-menu Use button's spell-routing
        // in `handle_context_menu_actions`.
        let stack_props = find_stack_by_type(&client_state, type_id).map(|s| s.properties);
        let resolved_spell = ObjectRegistry::resolved_spell_id_for_type(
            type_id,
            stack_props.as_ref(),
            &definitions,
            &spell_definitions,
        );
        let targeting_mode = resolved_spell
            .as_deref()
            .and_then(|id| spell_definitions.get(id))
            .map(|spell| spell.targeting);

        if matches!(
            targeting_mode,
            Some(SpellTargeting::Targeted | SpellTargeting::TargetedTile)
        ) {
            spell_targeting_state.source = Some(target);
            spell_targeting_state.spell_id = resolved_spell;
            cursor_state.mode = match targeting_mode {
                Some(SpellTargeting::TargetedTile) => CursorMode::SpellTargetTile,
                _ => CursorMode::SpellTarget,
            };
            cursor_state.use_on_sprite = None;
        } else if ctrl_held {
            use_on_state.source = Some(target);
            cursor_state.mode = CursorMode::UseOn;
            cursor_state.use_on_sprite = None;
        } else {
            pending_commands.push(GameCommand::UseItem {
                source: ItemReference::Slot(slot_ref),
            });
        }
        // Only act on one digit press per frame even if (rare) multiple were
        // marked `just_pressed`.
        break;
    }
}

/// Mouse handling for quickbar slots:
/// - **Right click** opens the standard inventory context menu for the
///   bound item, so the slot behaves like the underlying inventory slot.
/// - **Middle click** clears the binding.
///
/// Both events are consumed via `ButtonInput::clear_just_pressed` so they
/// don't fall through to `handle_context_menu_opening` (which would otherwise
/// open a world context menu for whatever tile the bar covers).
#[allow(clippy::too_many_arguments)]
pub fn handle_quickbar_clicks(
    mut mouse_input: ResMut<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    use_on_state: Res<UseOnState>,
    spell_targeting_state: Res<SpellTargetingState>,
    cursor_state: Res<CursorState>,
    slot_query: Query<
        (
            &QuickbarSlotMarker,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&Visibility>,
        ),
        With<Button>,
    >,
    mut quickbar: ResMut<Quickbar>,
    mut context_menu_state: ResMut<ContextMenuState>,
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
) {
    let right = mouse_input.just_pressed(MouseButton::Right);
    let middle = mouse_input.just_pressed(MouseButton::Middle);
    if !right && !middle {
        return;
    }
    // Defer to the targeting / context-menu handlers when any of those
    // modes are already active — an RMB in those states means "cancel",
    // not "open quickbar menu".
    if context_menu_state.is_visible()
        || use_on_state.source.is_some()
        || spell_targeting_state.source.is_some()
        || cursor_state.mode != CursorMode::Default
    {
        return;
    }
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Some(index) = hovered_quickbar_slot_idx(cursor_position, &slot_query) else {
        return;
    };

    if middle {
        quickbar.clear_slot(index);
        mouse_input.clear_just_pressed(MouseButton::Middle);
        return;
    }

    // Right click: consume the press so the world context-menu opener
    // (which runs after this system) doesn't also fire, then open the
    // standard inventory-slot context menu for the bound item. Bindings
    // with no live stack (dimmed icon) just consume the click.
    mouse_input.clear_just_pressed(MouseButton::Right);
    let Some(type_id) = quickbar.slots.get(index).and_then(|s| s.as_deref()) else {
        return;
    };
    let Some(slot_kind) = find_slot_kind_for_type(&client_state, type_id) else {
        return;
    };
    let Some(stack) = find_stack_by_type(&client_state, type_id) else {
        return;
    };
    let definition = definitions.get(type_id);
    let can_use = definition.is_some_and(|d| d.is_usable());
    let has_use_on = ObjectRegistry::resolved_spell_id_for_type(
        type_id,
        Some(&stack.properties),
        &definitions,
        &spell_definitions,
    )
    .is_some()
        || can_use;
    // Match `handle_context_menu_opening`: only Backpack-slot pouches show
    // "Open" — equipment-slot pouches are intentionally skipped.
    let can_open =
        matches!(slot_kind, ItemSlotKind::Backpack(_)) && stack.contained_slots.is_some();
    context_menu_state.show(
        cursor_position,
        ContextMenuTarget::Slot(slot_kind),
        can_open,
        can_use,
        has_use_on,
        false,
        stack.quantity > 1,
        false,
        false,
        None,
    );
}

fn hovered_quickbar_slot_idx<F: QueryFilter>(
    cursor_position: Vec2,
    slot_query: &Query<
        (
            &QuickbarSlotMarker,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&Visibility>,
        ),
        F,
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

fn point_in_ui_node(
    cursor_position: Vec2,
    computed_node: &ComputedNode,
    global_transform: &UiGlobalTransform,
) -> bool {
    let inv = computed_node.inverse_scale_factor();
    let physical_cursor = if inv > 0.0 {
        cursor_position / inv
    } else {
        cursor_position
    };
    computed_node.contains_point(*global_transform, physical_cursor)
}

/// Load slots from disk the first time we observe a fresh `local_player_id`.
pub fn load_quickbar_on_login(
    runtime: Res<AppRuntime>,
    client_state: Res<ClientGameState>,
    mut quickbar: ResMut<Quickbar>,
    mut loaded_for: ResMut<QuickbarLoadedFor>,
) {
    let Some(player_id) = client_state.local_player_id else {
        // Player logged out — reset so a future login reloads.
        if loaded_for.player_id.is_some() {
            *quickbar = Quickbar::default();
            loaded_for.player_id = None;
        }
        return;
    };

    if loaded_for.player_id == Some(player_id) {
        return;
    }

    let Some(path) = crate::app::paths::quickbar_path(*runtime, player_id.0) else {
        loaded_for.player_id = Some(player_id);
        return;
    };

    let next = read_quickbar_file(&path);
    quickbar.slots = next;
    quickbar.dirty = false;
    loaded_for.player_id = Some(player_id);
}

/// Write slots to disk when `dirty`. Skips when there's no logged-in player
/// (e.g. the user changed something at the title screen, which shouldn't
/// happen via the UI but defends against future edge cases).
pub fn persist_quickbar(
    runtime: Res<AppRuntime>,
    client_state: Res<ClientGameState>,
    mut quickbar: ResMut<Quickbar>,
) {
    if !quickbar.dirty {
        return;
    }
    let Some(player_id) = client_state.local_player_id else {
        return;
    };
    let Some(path) = crate::app::paths::quickbar_path(*runtime, player_id.0) else {
        quickbar.dirty = false;
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = QuickbarFile {
        slots: quickbar.slots.to_vec(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&file) {
        if let Err(err) = fs::write(&path, json) {
            warn!("failed to write quickbar file {}: {err}", path.display());
        }
    }
    quickbar.dirty = false;
}

fn read_quickbar_file(path: &PathBuf) -> [Option<String>; QUICKBAR_SLOT_COUNT] {
    let Ok(raw) = fs::read_to_string(path) else {
        return Default::default();
    };
    let Ok(parsed) = serde_json::from_str::<QuickbarFile>(&raw) else {
        return Default::default();
    };
    let mut out: [Option<String>; QUICKBAR_SLOT_COUNT] = Default::default();
    for (i, slot) in parsed
        .slots
        .into_iter()
        .take(QUICKBAR_SLOT_COUNT)
        .enumerate()
    {
        out[i] = slot;
    }
    out
}

/// Find the first stack in inventory (backpack first, then equipment slots)
/// matching `type_id`. Returns the matching `InventoryStack` if found.
fn find_stack_by_type(client_state: &ClientGameState, type_id: &str) -> Option<InventoryStack> {
    for slot in client_state.inventory.backpack_slots.iter().flatten() {
        if slot.type_id == type_id {
            return Some(slot.clone());
        }
    }
    for (slot, equipped) in &client_state.inventory.equipment_slots {
        if let Some(item) = equipped {
            if item.type_id == type_id {
                let qty = if *slot == crate::world::object_definitions::EquipmentSlot::Ammo {
                    client_state.inventory.ammo_quantity.max(1)
                } else {
                    1
                };
                return Some(InventoryStack::item(
                    item.type_id.clone(),
                    item.properties.clone(),
                    qty,
                ));
            }
        }
    }
    None
}

/// Find which slot currently holds an item of `type_id`. Backpack slots are
/// scanned first, then equipment slots — same order as `find_stack_by_type`.
fn find_slot_kind_for_type(client_state: &ClientGameState, type_id: &str) -> Option<ItemSlotKind> {
    for (index, slot) in client_state.inventory.backpack_slots.iter().enumerate() {
        if let Some(stack) = slot {
            if stack.type_id == type_id {
                return Some(ItemSlotKind::Backpack(index));
            }
        }
    }
    for (slot, equipped) in &client_state.inventory.equipment_slots {
        if let Some(item) = equipped {
            if item.type_id == type_id {
                return Some(ItemSlotKind::Equipment(*slot));
            }
        }
    }
    None
}

fn item_slot_kind_to_inventory_ref(slot_kind: ItemSlotKind) -> ItemSlotRef {
    match slot_kind {
        ItemSlotKind::Backpack(index) => ItemSlotRef::Backpack(index),
        ItemSlotKind::Equipment(slot) => ItemSlotRef::Equipment(slot),
        // The lookup helpers above only ever return Backpack or Equipment, so
        // these are unreachable in practice. If a future change starts indexing
        // pouches into the quickbar, extend `find_slot_kind_for_type`.
        _ => ItemSlotRef::Backpack(usize::MAX),
    }
}

/// Toggle the chat/console area visibility. The close-X button on the
/// chat header and the `F1` keybind both feed into `BottomPanelVisibility`.
pub fn handle_bottom_panel_hide_button(
    interactions: Query<&Interaction, (With<BottomPanelHideButton>, Changed<Interaction>)>,
    mut visibility: ResMut<BottomPanelVisibility>,
) {
    for interaction in &interactions {
        if matches!(interaction, Interaction::Pressed) {
            visibility.hidden = !visibility.hidden;
        }
    }
}

pub fn handle_bottom_panel_hide_key(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    keybindings: Res<crate::ui::settings::Keybindings>,
    console_state: Option<Res<PythonConsoleState>>,
    mut visibility: ResMut<BottomPanelVisibility>,
) {
    if console_state.as_ref().is_some_and(|s| s.is_open) {
        return;
    }
    if keybindings.just_pressed(
        crate::ui::settings::model::Action::ToggleBottomPanel,
        &keyboard_input,
    ) {
        visibility.hidden = !visibility.hidden;
    }
}

/// If the user opens the Python console while the chat area is hidden,
/// also un-hide the area so the console is actually visible. Without this,
/// the keystroke would silently flip `is_open` while both panels stay
/// `Display::None`.
pub fn unhide_on_console_open(
    console_state: Option<Res<PythonConsoleState>>,
    mut visibility: ResMut<BottomPanelVisibility>,
) {
    let Some(state) = console_state else { return };
    if state.is_open && visibility.hidden {
        visibility.hidden = false;
    }
}

/// Single owner of chat-area / chat-panel / console-panel `Display`.
/// Reads `BottomPanelVisibility::hidden` and `PythonConsoleState::is_open`,
/// then writes:
/// - `ChatAreaContainer` → `None` when hidden (so the quickbar drops to
///   the screen edge), `Flex` otherwise;
/// - the chat and console panels alternate inside the container based on
///   `is_open`.
pub fn sync_bottom_panels_visibility(
    visibility: Res<BottomPanelVisibility>,
    console_state: Option<Res<PythonConsoleState>>,
    mut chat_areas: Query<
        &mut Node,
        (
            With<ChatAreaContainer>,
            Without<ChatPanel>,
            Without<PythonConsolePanel>,
        ),
    >,
    mut chat_panels: Query<
        &mut Node,
        (
            With<ChatPanel>,
            Without<ChatAreaContainer>,
            Without<PythonConsolePanel>,
        ),
    >,
    mut console_panels: Query<
        &mut Node,
        (
            With<PythonConsolePanel>,
            Without<ChatAreaContainer>,
            Without<ChatPanel>,
        ),
    >,
) {
    let console_open = console_state.as_ref().is_some_and(|s| s.is_open);
    let area_display = if visibility.hidden {
        Display::None
    } else {
        Display::Flex
    };
    let chat_display = if console_open {
        Display::None
    } else {
        Display::Flex
    };
    let console_display = if console_open {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut chat_areas {
        if node.display != area_display {
            node.display = area_display;
        }
    }
    for mut node in &mut chat_panels {
        if node.display != chat_display {
            node.display = chat_display;
        }
    }
    for mut node in &mut console_panels {
        if node.display != console_display {
            node.display = console_display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::{EquippedItem, Inventory};
    use crate::world::map_layout::ObjectProperties;
    use crate::world::object_definitions::EquipmentSlot;

    fn state_with_inventory(inventory: Inventory) -> ClientGameState {
        ClientGameState {
            inventory,
            ..Default::default()
        }
    }

    #[test]
    fn type_id_resolves_to_backpack_slot_first() {
        let mut inventory = Inventory::default();
        inventory.backpack_slots[3] = Some(InventoryStack::item(
            "health_potion",
            ObjectProperties::new(),
            1,
        ));
        inventory.equipment_slots[0] = (
            EquipmentSlot::Weapon,
            Some(EquippedItem::new("health_potion")),
        );
        let state = state_with_inventory(inventory);
        let slot = find_slot_kind_for_type(&state, "health_potion").unwrap();
        assert_eq!(slot, ItemSlotKind::Backpack(3));
    }

    #[test]
    fn type_id_falls_back_to_equipment() {
        let mut inventory = Inventory::default();
        inventory.equipment_slots[0] = (
            EquipmentSlot::Weapon,
            Some(EquippedItem::new("wand_of_sparks")),
        );
        let state = state_with_inventory(inventory);
        let slot = find_slot_kind_for_type(&state, "wand_of_sparks").unwrap();
        assert_eq!(slot, ItemSlotKind::Equipment(EquipmentSlot::Weapon));
    }

    #[test]
    fn type_id_not_in_inventory_returns_none() {
        let state = state_with_inventory(Inventory::default());
        assert!(find_slot_kind_for_type(&state, "ghost_item").is_none());
    }

    #[test]
    fn quickbar_assign_marks_dirty() {
        let mut bar = Quickbar::default();
        bar.assign(0, "health_potion".to_owned());
        assert!(bar.dirty);
        assert_eq!(bar.slots[0].as_deref(), Some("health_potion"));

        // Same value -> not marked dirty again
        bar.dirty = false;
        bar.assign(0, "health_potion".to_owned());
        assert!(!bar.dirty);

        // Different value -> dirty again
        bar.assign(0, "mana_potion".to_owned());
        assert!(bar.dirty);
    }
}
