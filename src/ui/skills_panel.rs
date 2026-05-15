//! Skills panel: a `MovableWindow` that lists all 10 skills with current
//! rank, max rank, and a `+` button per row, plus the unspent skill-point
//! counter. Mirrors the `recipe_book` panel's lifecycle exactly — spawned by
//! the `KeyK` shortcut (or by the `OpenSkillsPanel` UI event); closing is
//! owned by the title-bar X.

use bevy::prelude::*;

use crate::app::state::{simulation_active, ClientAppState};
use crate::game::commands::GameCommand;
use crate::game::resources::{
    ClientGameState, GameUiEvent, PendingGameCommands, PendingGameUiEvents,
};
use crate::player::classes::Class;
use crate::player::skills::{is_class_skill, max_rank, rank_cost, Skill};
use crate::ui::movable_window::{
    find_window_by_id, spawn_movable_window, spawn_movable_window_close_button, MovableWindow,
    MovableWindowId, MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::theme::{Palette, UiThemeAssets};

#[derive(Component, Default, Clone, Debug)]
pub struct SkillsPanelRoot;

#[derive(Component)]
pub struct SkillsPanelContent;

#[derive(Component, Clone, Copy, Debug)]
pub struct AllocateSkillButton {
    pub skill: Skill,
}

const PANEL_SIZE: Vec2 = Vec2::new(360.0, 460.0);
const PANEL_INITIAL_POS: Vec2 = Vec2::new(140.0, 100.0);

#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SkillsPanelSystemSet {
    Process,
}

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        open_skills_panel_on_keybind
            .run_if(in_state(ClientAppState::InGame))
            .run_if(simulation_active)
            .run_if(bevy_terminal::terminal_not_focused)
            .in_set(SkillsPanelSystemSet::Process),
    )
    .add_systems(
        Update,
        (
            consume_open_skills_panel_event,
            rebuild_skills_panel_contents,
            handle_allocate_skill_button_clicks,
        )
            .chain()
            .in_set(SkillsPanelSystemSet::Process)
            .run_if(in_state(ClientAppState::InGame))
            .run_if(simulation_active),
    );
}

fn open_skills_panel_on_keybind(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    theme: Option<Res<UiThemeAssets>>,
    palette: Option<Res<Palette>>,
    windows: Query<(Entity, &MovableWindow)>,
) {
    if !keyboard.just_pressed(KeyCode::KeyK) {
        return;
    }
    if find_window_by_id(&windows, MovableWindowId::SkillsPanel).is_some() {
        return;
    }
    let Some(theme) = theme.as_deref() else {
        return;
    };
    let Some(palette) = palette.as_deref() else {
        return;
    };
    spawn_skills_panel(&mut commands, theme, palette);
}

fn consume_open_skills_panel_event(
    mut commands: Commands,
    mut pending: ResMut<PendingGameUiEvents>,
    theme: Option<Res<UiThemeAssets>>,
    palette: Option<Res<Palette>>,
    windows: Query<(Entity, &MovableWindow)>,
) {
    let events = std::mem::take(&mut pending.events);
    let mut keep = Vec::with_capacity(events.len());
    let mut should_open = false;
    for event in events {
        if matches!(event, GameUiEvent::OpenSkillsPanel) {
            should_open = true;
        } else {
            keep.push(event);
        }
    }
    pending.events = keep;

    if !should_open {
        return;
    }
    if find_window_by_id(&windows, MovableWindowId::SkillsPanel).is_some() {
        return;
    }
    let Some(theme) = theme.as_deref() else {
        return;
    };
    let Some(palette) = palette.as_deref() else {
        return;
    };
    spawn_skills_panel(&mut commands, theme, palette);
}

fn spawn_skills_panel(commands: &mut Commands, theme: &UiThemeAssets, palette: &Palette) {
    let spawned = spawn_movable_window(
        commands,
        theme,
        palette,
        MovableWindowId::SkillsPanel,
        "Skills",
        PANEL_SIZE,
        PANEL_INITIAL_POS,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );
    commands.entity(spawned.root).insert(SkillsPanelRoot);
    commands.entity(spawned.body).insert(SkillsPanelContent);
    commands.entity(spawned.title_bar).with_children(|bar| {
        spawn_movable_window_close_button(bar, theme, palette, spawned.root);
    });
}

fn rebuild_skills_panel_contents(
    mut commands: Commands,
    client_state: Res<ClientGameState>,
    palette: Option<Res<Palette>>,
    roots: Query<Ref<SkillsPanelRoot>>,
    content: Query<Entity, With<SkillsPanelContent>>,
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

    let class = client_state.class.unwrap_or(Class::Fighter);
    let level = client_state
        .experience
        .as_ref()
        .map(|e| e.level)
        .unwrap_or(1);
    let available_points = client_state.available_skill_points;

    commands.entity(body).with_children(|root| {
        root.spawn((
            Text::new(format!("Available points: {available_points}")),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(palette.text_accent),
            Node {
                margin: UiRect::bottom(Val::Px(6.0)),
                ..default()
            },
        ));

        for skill in Skill::ALL {
            let current_rank = client_state.skill_ranks[skill.index()];
            let cap = max_rank(class, skill, level);
            let cost = rank_cost(class, skill);
            let is_class = is_class_skill(class, skill);
            let can_afford = available_points >= cost;
            let at_cap = current_rank >= cap;
            let enabled = !at_cap && can_afford;

            let name_color = if is_class {
                palette.text_accent
            } else {
                palette.text_primary
            };

            root.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(palette.border_slot),
                BackgroundColor(palette.surface_raised),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(skill.label().to_owned()),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(name_color),
                    Node {
                        min_width: Val::Px(120.0),
                        ..default()
                    },
                ));
                row.spawn((
                    Text::new(format!("{}/{}", current_rank, cap)),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(palette.text_primary),
                    Node {
                        min_width: Val::Px(56.0),
                        ..default()
                    },
                ));
                row.spawn((
                    Text::new(format!("cost {cost}")),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(palette.text_muted),
                    Node {
                        min_width: Val::Px(56.0),
                        ..default()
                    },
                ));
                let button_bg = if enabled {
                    palette.surface_panel
                } else {
                    palette.surface_raised
                };
                row.spawn((
                    Button,
                    AllocateSkillButton { skill },
                    Node {
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(palette.border_accent),
                    BackgroundColor(button_bg),
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("+"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(palette.text_primary),
                    ));
                });
            });
        }
    });
}

fn handle_allocate_skill_button_clicks(
    mut pending_commands: ResMut<PendingGameCommands>,
    client_state: Res<ClientGameState>,
    interactions: Query<(&Interaction, &AllocateSkillButton), Changed<Interaction>>,
) {
    let class = client_state.class.unwrap_or(Class::Fighter);
    let level = client_state
        .experience
        .as_ref()
        .map(|e| e.level)
        .unwrap_or(1);
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // Client-side gate so a spammed click on a maxed-or-broke row doesn't
        // pile up rejected commands. Server still enforces both invariants.
        let current_rank = client_state.skill_ranks[button.skill.index()];
        let cap = max_rank(class, button.skill, level);
        let cost = rank_cost(class, button.skill);
        if current_rank >= cap || client_state.available_skill_points < cost {
            continue;
        }
        pending_commands.push(GameCommand::AllocateSkillPoint {
            skill: button.skill,
            ranks: 1,
        });
    }
}
