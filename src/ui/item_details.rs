//! Item details popup: a movable window that shows the sprite, name,
//! weight, description, stat modifiers, and use effects of an item in any
//! addressable slot (inventory / equipment / open container / pouch / trade).
//!
//! Opened by the context-menu "Inspect" action via the queue resource
//! [`PendingItemDetailsOpens`]. Lives entirely client-side — all data is read
//! from `ClientGameState` + `OverworldObjectDefinitions`, so there is no
//! server roundtrip and offline play behaves identically to networked play.

use bevy::prelude::*;
use bevy::text::{Justify, LineBreak, TextLayout};
use bevy::window::PrimaryWindow;

use crate::app::state::ClientAppState;
use crate::game::resources::ClientGameState;
use crate::magic::resources::SpellDefinitions;
use crate::player::components::InventoryStack;
use crate::ui::components::ItemSlotKind;
use crate::ui::movable_window::{
    find_window_by_id, spawn_movable_window, spawn_movable_window_close_button, MovableWindow,
    MovableWindowContent, MovableWindowDrag, MovableWindowId, MOVABLE_WINDOW_CASCADE_PX,
};
use crate::ui::resources::DockedPanelState;
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;

const ITEM_DETAILS_SIZE: Vec2 = Vec2::new(320.0, 380.0);

/// Marker on the body node of an item details window. Stores enough state to
/// detect when the underlying stack changes (rebuild children) or disappears
/// (despawn the whole window).
#[derive(Component)]
pub struct ItemDetailsContent {
    pub slot_kind: ItemSlotKind,
    pub last_rendered: Option<InventoryStack>,
}

/// One-shot queue for "open the details popup for this slot". The Inspect
/// context-menu handler pushes here; `handle_pending_item_details_opens`
/// drains it.
#[derive(Resource, Default)]
pub struct PendingItemDetailsOpens {
    pub slots: Vec<ItemSlotKind>,
}

pub struct ItemDetailsPlugin;

impl Plugin for ItemDetailsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PendingItemDetailsOpens::default())
            .add_systems(
                Update,
                (
                    handle_pending_item_details_opens,
                    sync_item_details_content.after(handle_pending_item_details_opens),
                )
                    .run_if(in_state(ClientAppState::InGame)),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_pending_item_details_opens(
    mut commands: Commands,
    mut pending: ResMut<PendingItemDetailsOpens>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    window_query: Query<(Entity, &MovableWindow)>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    if pending.slots.is_empty() {
        return;
    }
    let slots = std::mem::take(&mut pending.slots);
    let window_size = primary_window
        .single()
        .ok()
        .map(|window| Vec2::new(window.width(), window.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    let center = ((window_size - ITEM_DETAILS_SIZE) * 0.5).max(Vec2::ZERO);
    let max_pos = (window_size - ITEM_DETAILS_SIZE).max(Vec2::ZERO);

    for slot_kind in slots {
        let id = MovableWindowId::ItemDetails(slot_kind);
        if let Some(existing) = find_window_by_id(&window_query, id) {
            drag.focused = Some(existing);
            continue;
        }
        let Some(stack) =
            crate::ui::systems::stack_in_slot_kind(&client_state, &docked_panel_state, slot_kind)
        else {
            // Slot empty by the time the queue drained — drop it.
            continue;
        };

        let title = ObjectRegistry::display_name_for_type(
            &stack.type_id,
            Some(&stack.properties),
            &definitions,
            &spell_definitions,
        )
        .unwrap_or_else(|| stack.type_id.clone());

        let existing_count = window_query.iter().count();
        let cascade = MOVABLE_WINDOW_CASCADE_PX * existing_count as f32;
        let initial_pos = (center + Vec2::splat(cascade)).clamp(Vec2::ZERO, max_pos);

        let spawned = spawn_movable_window(
            &mut commands,
            &theme,
            &palette,
            id,
            &title,
            ITEM_DETAILS_SIZE,
            initial_pos,
        );

        let root = spawned.root;
        commands.entity(spawned.title_bar).with_children(|bar| {
            spawn_movable_window_close_button(bar, &theme, &palette, root);
        });
        commands.entity(spawned.body).insert(ItemDetailsContent {
            slot_kind,
            last_rendered: None,
        });
        drag.focused = Some(root);
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_item_details_content(
    mut commands: Commands,
    client_state: Res<ClientGameState>,
    docked_panel_state: Res<DockedPanelState>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    palette: Res<Palette>,
    asset_server: Res<AssetServer>,
    mut content_query: Query<(Entity, &MovableWindowContent, &mut ItemDetailsContent)>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    for (body_entity, body_marker, mut content) in &mut content_query {
        let stack = crate::ui::systems::stack_in_slot_kind(
            &client_state,
            &docked_panel_state,
            content.slot_kind,
        );

        let Some(stack) = stack else {
            commands.entity(body_marker.owner).despawn();
            if drag.focused == Some(body_marker.owner) {
                drag.focused = None;
            }
            if drag.dragging.is_some_and(|(e, _)| e == body_marker.owner) {
                drag.dragging = None;
            }
            continue;
        };

        if content.last_rendered.as_ref() == Some(&stack) {
            continue;
        }
        content.last_rendered = Some(stack.clone());

        let palette_snapshot = *palette;
        commands.entity(body_entity).despawn_related::<Children>();
        commands.entity(body_entity).with_children(|parent| {
            populate_item_details(
                parent,
                &palette_snapshot,
                &asset_server,
                &definitions,
                &spell_definitions,
                &stack,
            );
        });
    }
}

fn populate_item_details(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    asset_server: &AssetServer,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    stack: &InventoryStack,
) {
    let definition = definitions.get(&stack.type_id);

    // Header: sprite + name.
    parent
        .spawn((
            Node {
                width: percent(100.0),
                column_gap: px(10.0),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|header| {
            let sprite_size = px(64.0);
            let sprite_path =
                definition.and_then(|def| def.sprite_path_for_state_count(None, stack.quantity));
            if let Some(path) = sprite_path {
                header.spawn((
                    Node {
                        width: sprite_size,
                        height: sprite_size,
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ImageNode::new(asset_server.load(path.to_owned())),
                ));
            } else {
                // Fallback swatch from the debug color so unparented items
                // (e.g. assets still being authored) still have a visual.
                let color = definition.map_or(Color::srgb(0.4, 0.4, 0.4), |def| def.debug_color());
                header.spawn((
                    Node {
                        width: sprite_size,
                        height: sprite_size,
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(color),
                ));
            }

            let name = ObjectRegistry::display_name_for_type(
                &stack.type_id,
                Some(&stack.properties),
                definitions,
                spell_definitions,
            )
            .unwrap_or_else(|| stack.type_id.clone());

            let display = if stack.quantity > 1 {
                format!("{} x{}", name, stack.quantity)
            } else {
                name
            };
            header.spawn((
                Text::new(display),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(palette.text_primary),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });

    // Weight line.
    if let Some(def) = definition {
        if def.weight > 0.0 {
            let line = if stack.quantity > 1 {
                format!(
                    "Weight: {:.2} kg (x{} = {:.2} kg)",
                    def.weight,
                    stack.quantity,
                    def.weight * stack.quantity as f32
                )
            } else {
                format!("Weight: {:.2} kg", def.weight)
            };
            parent.spawn((
                Text::new(line),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(palette.text_muted),
            ));
        }
    }

    // Description.
    let description = ObjectRegistry::description_with_count_for_type(
        &stack.type_id,
        Some(&stack.properties),
        stack.quantity,
        definitions,
        spell_definitions,
    )
    .unwrap_or_default();
    if !description.trim().is_empty() {
        parent.spawn((
            Text::new(description),
            TextFont {
                font_size: 13.0,
                ..default()
            },
            TextColor(palette.text_value),
            TextLayout::new(Justify::Left, LineBreak::WordBoundary),
            Node {
                width: percent(100.0),
                ..default()
            },
        ));
    }

    // Stat modifiers (equipment).
    if let Some(def) = definition {
        let stats = &def.stats;
        let entries: Vec<(&str, i32)> = [
            ("Strength", stats.strength),
            ("Agility", stats.agility),
            ("Constitution", stats.constitution),
            ("Willpower", stats.willpower),
            ("Charisma", stats.charisma),
            ("Focus", stats.focus),
            ("Max Health", stats.max_health),
            ("Max Mana", stats.max_mana),
            ("Storage Slots", stats.storage_slots),
        ]
        .into_iter()
        .filter(|(_, v)| *v != 0)
        .collect();
        if !entries.is_empty() {
            spawn_section_header(parent, palette, "Stats");
            for (label, value) in entries {
                let sign = if value > 0 { "+" } else { "" };
                let color = if value > 0 {
                    Color::srgb(0.55, 0.85, 0.45)
                } else {
                    Color::srgb(0.95, 0.45, 0.40)
                };
                parent.spawn((
                    Text::new(format!("  {}{} {}", sign, value, label)),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(color),
                ));
            }
        }
    }

    // Use effects (consumables).
    if let Some(def) = definition {
        let effects = &def.use_effects;
        let mut lines: Vec<String> = Vec::new();
        if effects.restore_health > 0.0 {
            lines.push(format!("Restores {:.0} health", effects.restore_health));
        }
        if effects.restore_mana > 0.0 {
            lines.push(format!("Restores {:.0} mana", effects.restore_mana));
        }
        if effects.regen_duration_seconds > 0.0 && effects.regen_multiplier > 1.0 {
            lines.push(format!(
                "Regen x{:.1} for {:.0}s",
                effects.regen_multiplier, effects.regen_duration_seconds
            ));
        }
        if !lines.is_empty() {
            spawn_section_header(parent, palette, "On use");
            for line in lines {
                parent.spawn((
                    Text::new(format!("  {}", line)),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(palette.text_value),
                ));
            }
        }
    }
}

fn spawn_section_header(parent: &mut ChildSpawnerCommands, palette: &Palette, label: &str) {
    parent.spawn((
        Text::new(label.to_owned()),
        TextFont {
            font_size: 13.0,
            ..default()
        },
        TextColor(palette.text_accent),
        Node {
            margin: UiRect::top(px(4.0)),
            ..default()
        },
    ));
}
