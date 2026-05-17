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
use crate::magic::resources::{SpellDefinitions, SpellTargeting};
use crate::player::components::{InventoryStack, CHARGES_KEY};
use crate::ui::components::ItemSlotKind;
use crate::ui::movable_window::{
    find_window_by_id, spawn_movable_window, spawn_movable_window_close_button, MovableWindow,
    MovableWindowContent, MovableWindowDrag, MovableWindowId, MOVABLE_WINDOW_CASCADE_PX,
    MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::resources::DockedPanelState;
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::world::object_definitions::{OverworldObjectDefinition, OverworldObjectDefinitions};
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
            MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
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

    spawn_header(
        parent,
        palette,
        asset_server,
        definitions,
        spell_definitions,
        stack,
        definition,
    );

    if let Some(def) = definition {
        spawn_properties_section(parent, palette, def, stack);
        spawn_spell_section(parent, palette, def, spell_definitions);
        spawn_stat_modifiers_section(parent, palette, def);
        spawn_use_effects_section(parent, palette, def);
        spawn_on_hit_effects_section(parent, palette, def);
    }

    spawn_description_section(parent, palette, definitions, spell_definitions, stack);
}

fn spawn_header(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    asset_server: &AssetServer,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    stack: &InventoryStack,
    definition: Option<&OverworldObjectDefinition>,
) {
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
}

fn spawn_properties_section(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    def: &OverworldObjectDefinition,
    stack: &InventoryStack,
) {
    let mut rows: Vec<(String, String)> = Vec::new();

    if let Some(type_label) = classify_item(def) {
        rows.push(("Type".to_owned(), type_label.to_owned()));
    }
    if let Some(slot) = def.equipment_slot {
        rows.push(("Slot".to_owned(), slot.label().to_owned()));
    }
    if def.weight > 0.0 {
        let value = if stack.quantity > 1 {
            format!(
                "{:.2} kg (×{} = {:.2} kg)",
                def.weight,
                stack.quantity,
                def.weight * stack.quantity as f32
            )
        } else {
            format!("{:.2} kg", def.weight)
        };
        rows.push(("Weight".to_owned(), value));
    }
    if let Some(damage) = def.damage.as_deref() {
        rows.push(("Damage".to_owned(), damage.to_owned()));
    }
    if let Some(profile) = def.attack_profile.as_ref() {
        let kind_label = match profile.kind {
            crate::world::object_definitions::AttackProfileKindDef::Melee => "Melee",
            crate::world::object_definitions::AttackProfileKindDef::Ranged => "Ranged",
        };
        rows.push(("Attack".to_owned(), kind_label.to_owned()));
        if let Some(dmg_type) = profile.damage_type {
            rows.push((
                "Damage Type".to_owned(),
                title_case(dmg_type.display_name()),
            ));
        }
    }
    if let Some(range) = def.base_range_tiles {
        rows.push(("Range".to_owned(), format!("{range} tiles")));
    }
    if let Some(ammo) = def.ammo_type.as_deref() {
        rows.push(("Ammo".to_owned(), human_id(ammo)));
    }
    if def.armor > 0 {
        rows.push(("Armor".to_owned(), format!("+{}", def.armor)));
    }
    if def.block > 0 {
        rows.push(("Block".to_owned(), format!("+{}", def.block)));
    }
    if def.infinite_uses {
        rows.push(("Charges".to_owned(), "Unlimited".to_owned()));
    } else if let Some(max) = def.max_charges {
        let current = stack
            .properties
            .get(CHARGES_KEY)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(max);
        rows.push(("Charges".to_owned(), format!("{current} / {max}")));
    }
    if def.max_stack_size > 1 {
        rows.push((
            "Stack".to_owned(),
            format!("{} / {}", stack.quantity, def.max_stack_size),
        ));
    }
    if let Some(cap) = def.container_capacity {
        rows.push(("Capacity".to_owned(), format!("{cap} slots")));
    }

    if rows.is_empty() {
        return;
    }
    spawn_section_header(parent, palette, "Properties");
    spawn_property_table(parent, palette, &rows);
}

fn spawn_spell_section(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    def: &OverworldObjectDefinition,
    spell_definitions: &SpellDefinitions,
) {
    let Some(spell_id) = def.spell_id.as_deref() else {
        return;
    };
    let Some(spell) = spell_definitions.get(spell_id) else {
        return;
    };

    spawn_section_header(parent, palette, "Spell");
    let mut rows: Vec<(String, String)> = vec![("Name".to_owned(), spell.name.clone())];
    let targeting = match spell.targeting {
        SpellTargeting::Targeted => "Targeted",
        SpellTargeting::Untargeted => "Self",
    };
    rows.push(("Targeting".to_owned(), targeting.to_owned()));
    rows.push(("Mana Cost".to_owned(), format!("{:.0}", spell.mana_cost)));
    if spell.targeting == SpellTargeting::Targeted && spell.range_tiles > 0 {
        rows.push(("Range".to_owned(), format!("{} tiles", spell.range_tiles)));
    }
    if spell.min_caster_level > 1 {
        rows.push((
            "Min Level".to_owned(),
            format!("{}", spell.min_caster_level),
        ));
    }
    let effects = &spell.effects;
    if effects.damage > 0.0 {
        let dmg_type = effects.effective_damage_type();
        rows.push((
            "Damage".to_owned(),
            format!("{:.0} {}", effects.damage, dmg_type.display_name()),
        ));
    }
    if effects.restore_health > 0.0 {
        rows.push((
            "Restores HP".to_owned(),
            format!("{:.0}", effects.restore_health),
        ));
    }
    if effects.restore_mana > 0.0 {
        rows.push((
            "Restores MP".to_owned(),
            format!("{:.0}", effects.restore_mana),
        ));
    }
    spawn_property_table(parent, palette, &rows);

    // Surface timed self-buffs and target-debuffs that the spell applies.
    if !effects.buffs_self.is_empty() {
        let lines: Vec<String> = effects
            .buffs_self
            .iter()
            .map(|spec| {
                format!(
                    "  {:?} {:.1} for {:.0}s",
                    spec.kind, spec.magnitude, spec.seconds
                )
            })
            .collect();
        spawn_indented_lines(parent, palette, "Buffs (self)", &lines);
    }
    if !effects.buffs_target.is_empty() {
        let lines: Vec<String> = effects
            .buffs_target
            .iter()
            .map(|spec| {
                format!(
                    "  {:?} {:.1} for {:.0}s",
                    spec.kind, spec.magnitude, spec.seconds
                )
            })
            .collect();
        spawn_indented_lines(parent, palette, "Debuffs (target)", &lines);
    }
}

fn spawn_stat_modifiers_section(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    def: &OverworldObjectDefinition,
) {
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
    if entries.is_empty() {
        return;
    }
    spawn_section_header(parent, palette, "Stat Modifiers");
    for (label, value) in entries {
        let sign = if value > 0 { "+" } else { "" };
        let color = if value > 0 {
            Color::srgb(0.55, 0.85, 0.45)
        } else {
            Color::srgb(0.95, 0.45, 0.40)
        };
        spawn_colored_property_row(
            parent,
            palette,
            label,
            &format!("{}{}", sign, value),
            color,
        );
    }
}

fn spawn_use_effects_section(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    def: &OverworldObjectDefinition,
) {
    let effects = &def.use_effects;
    let mut rows: Vec<(String, String)> = Vec::new();
    if effects.restore_health > 0.0 {
        rows.push((
            "Restores HP".to_owned(),
            format!("{:.0}", effects.restore_health),
        ));
    }
    if effects.restore_mana > 0.0 {
        rows.push((
            "Restores MP".to_owned(),
            format!("{:.0}", effects.restore_mana),
        ));
    }
    if effects.regen_duration_seconds > 0.0 && effects.regen_multiplier > 1.0 {
        rows.push((
            "Regen".to_owned(),
            format!(
                "×{:.1} for {:.0}s",
                effects.regen_multiplier, effects.regen_duration_seconds
            ),
        ));
    }
    if rows.is_empty() {
        return;
    }
    spawn_section_header(parent, palette, "On Use");
    spawn_property_table(parent, palette, &rows);
}

fn spawn_on_hit_effects_section(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    def: &OverworldObjectDefinition,
) {
    let Some(profile) = def.attack_profile.as_ref() else {
        return;
    };
    if profile.on_hit_effects.is_empty() {
        return;
    }
    spawn_section_header(parent, palette, "On Hit");
    for effect in &profile.on_hit_effects {
        let chance_pct = (effect.chance * 100.0).round() as i32;
        let line = format!(
            "  {}% {:?} {:.1} for {:.0}s",
            chance_pct, effect.kind, effect.magnitude, effect.seconds
        );
        parent.spawn((
            Text::new(line),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(palette.text_value),
        ));
    }
}

fn spawn_description_section(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    stack: &InventoryStack,
) {
    let description = ObjectRegistry::description_with_count_for_type(
        &stack.type_id,
        Some(&stack.properties),
        stack.quantity,
        definitions,
        spell_definitions,
    )
    .unwrap_or_default();
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return;
    }
    spawn_section_header(parent, palette, "Description");
    parent.spawn((
        Text::new(trimmed.to_owned()),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(palette.text_muted),
        TextLayout::new(Justify::Left, LineBreak::WordBoundary),
        Node {
            width: percent(100.0),
            ..default()
        },
    ));
}

/// Render a list of `(label, value)` pairs as a vertical stack of two-column
/// rows. Label sits in a fixed-width column tinted muted-accent; value flows
/// to the right tinted value-primary. Keeps the inspect popup scannable.
fn spawn_property_table(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    rows: &[(String, String)],
) {
    for (label, value) in rows {
        spawn_property_row(parent, palette, label, value);
    }
}

fn spawn_property_row(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    label: &str,
    value: &str,
) {
    spawn_colored_property_row(parent, palette, label, value, palette.text_value);
}

fn spawn_colored_property_row(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    label: &str,
    value: &str,
    value_color: Color,
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                column_gap: px(8.0),
                align_items: AlignItems::FlexStart,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(palette.text_muted),
                Node {
                    width: px(108.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row.spawn((
                Text::new(value.to_owned()),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(value_color),
                TextLayout::new(Justify::Left, LineBreak::WordBoundary),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}

fn spawn_indented_lines(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    label: &str,
    lines: &[String],
) {
    parent.spawn((
        Text::new(label.to_owned()),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(palette.text_muted),
        Node {
            margin: UiRect::top(px(2.0)),
            ..default()
        },
    ));
    for line in lines {
        parent.spawn((
            Text::new(line.to_owned()),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(palette.text_value),
        ));
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
            margin: UiRect::top(px(6.0)),
            ..default()
        },
    ));
}

/// Classify the item into a short human label based on the definition fields.
/// Designed to surface the most useful tag for the user — weapons are tagged
/// "Weapon" even when they're technically equipment, scrolls vs wands branch on
/// `max_charges`, etc.
fn classify_item(def: &OverworldObjectDefinition) -> Option<&'static str> {
    if def.attack_profile.is_some() {
        return Some("Weapon");
    }
    if def.equipment_slot.is_some() {
        return Some("Equipment");
    }
    if def.spell_id.is_some() {
        return Some(if def.max_charges.is_some() || def.infinite_uses {
            "Wand"
        } else {
            "Scroll"
        });
    }
    if def.container_capacity.is_some() {
        return Some("Container");
    }
    let ue = &def.use_effects;
    if ue.restore_health > 0.0 || ue.restore_mana > 0.0 || ue.regen_duration_seconds > 0.0 {
        return Some("Consumable");
    }
    if def.infinite_uses {
        return Some("Tool");
    }
    if def.learns_recipe.is_some() {
        return Some("Recipe Scroll");
    }
    None
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Convert a snake_case type_id into a readable label: "arrow" → "Arrow",
/// "iron_ingot" → "Iron Ingot".
fn human_id(s: &str) -> String {
    s.split('_')
        .map(title_case)
        .collect::<Vec<_>>()
        .join(" ")
}
