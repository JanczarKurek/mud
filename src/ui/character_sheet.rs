//! Character sheet: a `MovableWindow` showing class/level/XP, vitals,
//! attributes with their ability modifiers, and active status effects.
//! Mirrors the `skills_panel` lifecycle — toggled by `KeyC` *or* the HUD
//! portrait button (either trigger opens and closes it); the title-bar X
//! also closes it. Contents rebuild whenever `ClientGameState` changes so
//! the sheet stays current while open.

use bevy::prelude::*;

use crate::app::state::{simulation_active, ClientAppState};
use crate::game::resources::ClientGameState;
use crate::player::classes::ability_mod;
use crate::ui::components::CharacterSheetButton;
use crate::ui::movable_window::{
    find_window_by_id, spawn_movable_window, spawn_movable_window_close_button, MovableWindow,
    MovableWindowId, MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::theme::{Palette, UiThemeAssets};

#[derive(Component, Default, Clone, Debug)]
pub struct CharacterSheetRoot;

#[derive(Component)]
pub struct CharacterSheetContent;

const PANEL_SIZE: Vec2 = Vec2::new(460.0, 420.0);
const PANEL_INITIAL_POS: Vec2 = Vec2::new(160.0, 90.0);

#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CharacterSheetSystemSet {
    Process,
}

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        toggle_character_sheet_on_keybind
            .run_if(in_state(ClientAppState::InGame))
            .run_if(simulation_active)
            .run_if(bevy_terminal::terminal_not_focused)
            .in_set(CharacterSheetSystemSet::Process),
    )
    .add_systems(
        Update,
        (
            toggle_character_sheet_on_button,
            rebuild_character_sheet_contents,
        )
            .chain()
            .in_set(CharacterSheetSystemSet::Process)
            .run_if(in_state(ClientAppState::InGame)),
    );
}

/// `KeyC` toggles the character sheet — opens if closed, closes if open.
fn toggle_character_sheet_on_keybind(
    keyboard: Res<ButtonInput<KeyCode>>,
    keybindings: Res<crate::ui::settings::Keybindings>,
    mut commands: Commands,
    theme: Option<Res<UiThemeAssets>>,
    palette: Option<Res<Palette>>,
    windows: Query<(Entity, &MovableWindow)>,
) {
    if !keybindings.just_pressed(
        crate::ui::settings::model::Action::ToggleCharacterSheet,
        &keyboard,
    ) {
        return;
    }
    toggle_character_sheet(
        &mut commands,
        theme.as_deref(),
        palette.as_deref(),
        &windows,
    );
}

/// The HUD portrait button is a second toggle trigger, equivalent to `KeyC`.
fn toggle_character_sheet_on_button(
    interactions: Query<&Interaction, (Changed<Interaction>, With<CharacterSheetButton>)>,
    mut commands: Commands,
    theme: Option<Res<UiThemeAssets>>,
    palette: Option<Res<Palette>>,
    windows: Query<(Entity, &MovableWindow)>,
) {
    if !interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed))
    {
        return;
    }
    toggle_character_sheet(
        &mut commands,
        theme.as_deref(),
        palette.as_deref(),
        &windows,
    );
}

/// Open the sheet if no window exists, otherwise despawn the existing one.
fn toggle_character_sheet(
    commands: &mut Commands,
    theme: Option<&UiThemeAssets>,
    palette: Option<&Palette>,
    windows: &Query<(Entity, &MovableWindow)>,
) {
    if let Some(existing) = find_window_by_id(windows, MovableWindowId::CharacterSheet) {
        commands.entity(existing).despawn();
        return;
    }
    let Some(theme) = theme else {
        return;
    };
    let Some(palette) = palette else {
        return;
    };
    spawn_character_sheet(commands, theme, palette);
}

fn spawn_character_sheet(commands: &mut Commands, theme: &UiThemeAssets, palette: &Palette) {
    let spawned = spawn_movable_window(
        commands,
        theme,
        palette,
        MovableWindowId::CharacterSheet,
        "Character",
        PANEL_SIZE,
        PANEL_INITIAL_POS,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );
    // `HudRoot` so `teardown_hud` reaps the window on logout, matching the
    // log-panel precedent.
    commands
        .entity(spawned.root)
        .insert((CharacterSheetRoot, crate::ui::components::HudRoot));
    commands.entity(spawned.body).insert(CharacterSheetContent);
    commands.entity(spawned.title_bar).with_children(|bar| {
        spawn_movable_window_close_button(bar, theme, palette, spawned.root);
    });
}

/// Rebuild the sheet body whenever the window appears or the underlying
/// `ClientGameState` changes (XP, vitals, attributes, buffs). Mirrors
/// `rebuild_skills_panel_contents`.
fn rebuild_character_sheet_contents(
    mut commands: Commands,
    client_state: Res<ClientGameState>,
    palette: Option<Res<Palette>>,
    roots: Query<Ref<CharacterSheetRoot>>,
    content: Query<Entity, With<CharacterSheetContent>>,
) {
    let Ok(root_ref) = roots.single() else {
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

    let class_label = client_state
        .class
        .map(|c| c.label())
        .unwrap_or("Adventurer");
    let level_line = match &client_state.experience {
        Some(view) => match view.xp_for_next {
            Some(span) => format!(
                "Level {} {} - {}/{} XP",
                view.level, class_label, view.xp_into_level, span
            ),
            None => format!("Level {} {} - max level", view.level, class_label),
        },
        None => class_label.to_owned(),
    };

    let palette = *palette;
    commands.entity(body).with_children(|panel| {
        panel.spawn((
            Text::new(level_line),
            TextFont {
                font_size: 15.0,
                ..default()
            },
            TextColor(palette.text_primary),
        ));

        if let Some(v) = client_state.player_vitals {
            panel.spawn((
                Text::new(format!(
                    "HP {:.0} / {:.0}    MP {:.0} / {:.0}",
                    v.health, v.max_health, v.mana, v.max_mana
                )),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(palette.text_muted),
            ));
        }

        panel.spawn((
            Node {
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            Text::new("Attributes"),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(palette.text_accent),
        ));

        if let Some(attrs) = client_state.attributes {
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
                            column_gap: Val::Px(8.0),
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
                            TextColor(palette.text_muted),
                        ));
                        row.spawn((
                            Text::new(format!("{value}  [{mod_str}]")),
                            TextFont {
                                font_size: 14.0,
                                ..default()
                            },
                            TextColor(palette.text_primary),
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
                TextColor(palette.text_muted),
            ));
        }

        panel.spawn((
            Node {
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            Text::new("Status Effects"),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(palette.text_accent),
        ));

        let effect_text = match client_state.regen_buff {
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
            TextColor(palette.text_muted),
        ));
    });
}
