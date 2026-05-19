//! Recipe-book panel: a `MovableWindow` that lists every learned recipe
//! with input availability indicators and a Craft button per row. Toggled
//! by `KeyR` (press to open, press again to close), or force-opened with a
//! station filter via the `OpenRecipeBook` UI event (right-click → Craft).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::app::state::{simulation_active, ClientAppState};
use crate::crafting::recipes::RecipeDefinitions;
use crate::game::commands::GameCommand;
use crate::game::resources::{
    ClientGameState, GameUiEvent, PendingGameCommands, PendingGameUiEvents,
};
use crate::ui::movable_window::{
    find_window_by_id, spawn_movable_window, spawn_movable_window_close_button, MovableWindow,
    MovableWindowId, MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Marker + persistent state carried on the recipe-book window root. The
/// `filter_station` field drives the in-window recipe filter for
/// right-click → "Craft" flows; `None` shows every learned recipe.
#[derive(Component, Default, Clone, Debug)]
pub struct RecipeBookRoot {
    pub filter_station: Option<String>,
}

#[derive(Component)]
pub struct RecipeBookContent;

#[derive(Component, Clone, Debug)]
pub struct CraftButton {
    pub recipe_id: String,
}

const PANEL_SIZE: Vec2 = Vec2::new(360.0, 420.0);
const PANEL_INITIAL_POS: Vec2 = Vec2::new(120.0, 80.0);

/// Plugin module-private system-set: groups recipe-book wiring so the
/// CraftingClientPlugin can configure ordering with one set name.
#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RecipeBookSystemSet {
    Process,
}

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        toggle_recipe_book_on_keybind
            .run_if(in_state(ClientAppState::InGame))
            .run_if(simulation_active)
            .run_if(bevy_terminal::terminal_not_focused)
            .in_set(RecipeBookSystemSet::Process),
    )
    .add_systems(
        Update,
        (
            consume_open_recipe_book_event,
            rebuild_recipe_book_contents,
            handle_craft_button_clicks,
        )
            .chain()
            .in_set(RecipeBookSystemSet::Process)
            .run_if(in_state(ClientAppState::InGame))
            .run_if(simulation_active),
    );
}

/// `KeyR` toggles the recipe book — opens if closed, closes if open. The
/// title-bar X still closes it too. (The only other `KeyR` consumer is the
/// floor-viewer dev mode, which runs outside `InGame`, so they don't
/// collide.)
fn toggle_recipe_book_on_keybind(
    keyboard: Res<ButtonInput<KeyCode>>,
    keybindings: Res<crate::ui::settings::Keybindings>,
    mut commands: Commands,
    theme: Option<Res<UiThemeAssets>>,
    palette: Option<Res<Palette>>,
    windows: Query<(Entity, &MovableWindow)>,
) {
    if !keybindings.just_pressed(
        crate::ui::settings::model::Action::ToggleRecipeBook,
        &keyboard,
    ) {
        return;
    }
    if let Some(existing) = find_window_by_id(&windows, MovableWindowId::RecipeBook) {
        commands.entity(existing).despawn();
        return;
    }
    let Some(theme) = theme.as_deref() else {
        return;
    };
    let Some(palette) = palette.as_deref() else {
        return;
    };
    spawn_recipe_book(&mut commands, theme, palette, None);
}

/// Listens for `OpenRecipeBook { filter_station }` UI events (e.g. when
/// the player right-clicks a crafting station). If a window is already
/// open, despawn it first so the new filter applies cleanly.
fn consume_open_recipe_book_event(
    mut commands: Commands,
    mut pending: ResMut<PendingGameUiEvents>,
    theme: Option<Res<UiThemeAssets>>,
    palette: Option<Res<Palette>>,
    windows: Query<(Entity, &MovableWindow)>,
) {
    let events = std::mem::take(&mut pending.events);
    let mut keep = Vec::with_capacity(events.len());
    let mut new_filter: Option<Option<String>> = None;
    for event in events {
        if let GameUiEvent::OpenRecipeBook { filter_station } = &event {
            new_filter = Some(filter_station.clone());
        } else {
            keep.push(event);
        }
    }
    pending.events = keep;

    let Some(filter) = new_filter else {
        return;
    };
    if let Some(existing) = find_window_by_id(&windows, MovableWindowId::RecipeBook) {
        commands.entity(existing).despawn();
    }
    let Some(theme) = theme.as_deref() else {
        return;
    };
    let Some(palette) = palette.as_deref() else {
        return;
    };
    spawn_recipe_book(&mut commands, theme, palette, filter);
}

fn spawn_recipe_book(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    filter_station: Option<String>,
) {
    let spawned = spawn_movable_window(
        commands,
        theme,
        palette,
        MovableWindowId::RecipeBook,
        "Recipes",
        PANEL_SIZE,
        PANEL_INITIAL_POS,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );
    commands
        .entity(spawned.root)
        .insert(RecipeBookRoot { filter_station });
    commands.entity(spawned.body).insert(RecipeBookContent);
    commands.entity(spawned.title_bar).with_children(|bar| {
        spawn_movable_window_close_button(bar, theme, palette, spawned.root);
    });
}

/// Rebuilds the recipe-row children whenever the window appears, its
/// filter changes, the learned-recipe set changes, or the local
/// inventory changes. Inventory changes flip ingredient availability
/// indicators between primary and muted text colors.
fn rebuild_recipe_book_contents(
    mut commands: Commands,
    client_state: Res<ClientGameState>,
    recipe_defs: Res<RecipeDefinitions>,
    object_defs: Res<OverworldObjectDefinitions>,
    palette: Option<Res<Palette>>,
    roots: Query<(&RecipeBookRoot, Ref<RecipeBookRoot>)>,
    content: Query<Entity, With<RecipeBookContent>>,
) {
    let Ok((root, root_ref)) = roots.single() else {
        return;
    };
    if !root_ref.is_changed() && !client_state.is_changed() {
        return;
    }
    let Some(palette) = palette.as_deref() else {
        return;
    };
    let Ok(body) = content.single() else {
        return;
    };

    commands.entity(body).despawn_related::<Children>();

    let inventory_counts = collect_inventory_counts(&client_state);
    let mut ids: Vec<&str> = client_state
        .learned_recipes
        .iter()
        .map(String::as_str)
        .collect();
    ids.sort();

    if let Some(filter) = root.filter_station.as_ref() {
        let allowed: std::collections::HashSet<&str> = recipe_defs
            .by_station(filter)
            .iter()
            .map(String::as_str)
            .collect();
        ids.retain(|id| allowed.contains(id));
    }

    if ids.is_empty() {
        commands.entity(body).with_children(|parent| {
            parent.spawn((
                Text::new(if root.filter_station.is_some() {
                    "No learned recipes for this station yet.".to_owned()
                } else {
                    "You haven't learned any recipes yet.".to_owned()
                }),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(palette.text_muted),
            ));
        });
        return;
    }

    for recipe_id in ids {
        let Some(recipe) = recipe_defs.get(recipe_id) else {
            continue;
        };
        let can_craft = recipe.inputs.iter().all(|input| {
            inventory_counts
                .get(input.type_id.as_str())
                .copied()
                .unwrap_or(0)
                >= input.count
        });
        commands.entity(body).with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(6.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                BorderColor::all(palette.border_slot),
                BackgroundColor(palette.surface_raised),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(recipe.name.clone()),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(palette.text_accent),
                ));
                for input in &recipe.inputs {
                    let have = inventory_counts
                        .get(input.type_id.as_str())
                        .copied()
                        .unwrap_or(0);
                    let item_name = object_defs
                        .get(&input.type_id)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| input.type_id.clone());
                    let color = if have >= input.count {
                        palette.text_primary
                    } else {
                        palette.text_muted
                    };
                    row.spawn((
                        Text::new(format!("  {} × {} (have {})", input.count, item_name, have)),
                        TextFont {
                            font_size: 12.0,
                            ..default()
                        },
                        TextColor(color),
                    ));
                }
                if let Some(station) = recipe.station.as_ref() {
                    let station_name = object_defs
                        .get(station)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| station.clone());
                    row.spawn((
                        Text::new(format!("Requires nearby: {}", station_name)),
                        TextFont {
                            font_size: 12.0,
                            ..default()
                        },
                        TextColor(palette.text_muted),
                    ));
                }
                let button_bg = if can_craft {
                    palette.surface_panel
                } else {
                    palette.surface_raised
                };
                row.spawn((
                    Button,
                    CraftButton {
                        recipe_id: recipe_id.to_owned(),
                    },
                    Node {
                        align_self: AlignSelf::FlexStart,
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(palette.border_accent),
                    BackgroundColor(button_bg),
                ))
                .with_children(|button| {
                    button.spawn((
                        Text::new("Craft"),
                        TextFont {
                            font_size: 13.0,
                            ..default()
                        },
                        TextColor(palette.text_primary),
                    ));
                });
            });
        });
    }
}

/// Sends `GameCommand::CraftItem` when a Craft button is pressed.
fn handle_craft_button_clicks(
    mut pending_commands: ResMut<PendingGameCommands>,
    interactions: Query<(&Interaction, &CraftButton), Changed<Interaction>>,
) {
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        pending_commands.push(GameCommand::CraftItem {
            recipe_id: button.recipe_id.clone(),
        });
    }
}

/// Aggregate the client-side inventory snapshot by `type_id`. Pouch
/// contents are intentionally NOT counted today — the server's craft
/// validator only looks at backpack slots, so the UI mirrors that.
fn collect_inventory_counts(state: &ClientGameState) -> HashMap<&str, u32> {
    let mut totals: HashMap<&str, u32> = HashMap::new();
    for stack in state.inventory.backpack_slots.iter().flatten() {
        *totals.entry(stack.type_id.as_str()).or_insert(0) += stack.quantity;
    }
    totals
}
