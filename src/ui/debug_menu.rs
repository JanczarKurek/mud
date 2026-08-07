//! Debug/GM tools panel: a `MovableWindow` exposing invincibility, full heal,
//! level/XP controls, noclip, and teleport-to-cursor. Opened from the Debug
//! menu's "GM Tools" entry, which only appears when the game is launched with
//! `--debug` (see `menu_bar.rs`).
//!
//! Each button pushes an `Admin*` `GameCommand` through `PendingGameCommands`
//! (resolved server-side to the local player). God-mode / noclip state is
//! tracked optimistically client-side in [`DebugMenuState`] for the toggle
//! labels — the server narrates the authoritative effect to chat.

use bevy::prelude::*;

use crate::app::state::{simulation_active, ClientAppState};
use crate::game::commands::GameCommand;
use crate::game::resources::{ClientGameState, PendingGameCommands};
use crate::player::progression::LEVEL_CAP;
use crate::ui::movable_window::{
    find_window_by_id, spawn_standard_window, MovableWindow, MovableWindowId,
};
use crate::ui::resources::HoveredTile;
use crate::ui::theme::widgets::{spawn_themed_button, ButtonStyle};
use crate::ui::theme::{Palette, UiThemeAssets};

const XP_GRANT_STEP: u64 = 1000;
const PANEL_SIZE: Vec2 = Vec2::new(260.0, 300.0);
const PANEL_INITIAL_POS: Vec2 = Vec2::new(160.0, 120.0);

/// Optimistic client-side view of the GM toggles, used only to label the
/// buttons. The authoritative markers live on the server player entity.
#[derive(Resource, Default)]
pub struct DebugMenuState {
    god_mode: bool,
    noclip: bool,
    /// When true, the next left-click on a world tile teleports the player
    /// there. Armed by the "Teleport to cursor" button (the cursor is over the
    /// button at click time, so we can't teleport immediately).
    arming_teleport: bool,
}

#[derive(Component)]
struct DebugMenuRoot;

#[derive(Component)]
struct DebugMenuContent;

#[derive(Component, Clone, Copy, Debug)]
enum DebugMenuButton {
    ToggleGodMode,
    ToggleNoclip,
    FullHeal,
    LevelUp,
    LevelDown,
    GrantXp,
    TeleportToCursor,
}

pub fn register(app: &mut App) {
    app.init_resource::<DebugMenuState>()
        .add_systems(
            Update,
            rebuild_debug_menu_contents
                .run_if(in_state(ClientAppState::InGame))
                .run_if(simulation_active),
        )
        // Must run `.before(CommandIntercept)` so the `Admin*` commands it pushes
        // are drained by the intercept handlers the same frame. Without this the
        // commands reach `process_game_commands`, which discards admin commands.
        .add_systems(
            Update,
            (handle_debug_menu_clicks, handle_teleport_arming)
                .before(crate::game::CommandIntercept)
                .run_if(in_state(ClientAppState::InGame))
                .run_if(simulation_active),
        );
}

/// Open the GM panel if closed, close it if open. Called from the Debug menu's
/// "GM Tools" action. No-op if theme/palette aren't loaded yet.
pub fn toggle_gm_panel(
    commands: &mut Commands,
    theme: Option<&UiThemeAssets>,
    palette: Option<&Palette>,
    windows: &Query<(Entity, &MovableWindow)>,
) {
    if let Some(existing) = find_window_by_id(windows, MovableWindowId::DebugMenu) {
        commands.entity(existing).despawn();
        return;
    }
    let (Some(theme), Some(palette)) = (theme, palette) else {
        return;
    };
    spawn_standard_window(
        commands,
        theme,
        palette,
        MovableWindowId::DebugMenu,
        "GM Tools",
        PANEL_SIZE,
        PANEL_INITIAL_POS,
        DebugMenuRoot,
        DebugMenuContent,
    );
}

#[derive(Clone, Copy, PartialEq)]
struct DebugMenuSnapshot {
    god_mode: bool,
    noclip: bool,
    arming_teleport: bool,
    level: u32,
}

#[allow(clippy::too_many_arguments)]
fn rebuild_debug_menu_contents(
    mut commands: Commands,
    state: Res<DebugMenuState>,
    client_state: Res<ClientGameState>,
    palette: Option<Res<Palette>>,
    theme: Option<Res<UiThemeAssets>>,
    roots: Query<Ref<DebugMenuRoot>>,
    content: Query<Entity, With<DebugMenuContent>>,
    mut last: Local<Option<DebugMenuSnapshot>>,
) {
    let Ok(root_ref) = roots.single() else {
        return;
    };
    let level = client_state
        .experience
        .as_ref()
        .map(|e| e.level)
        .unwrap_or(1);
    let want = DebugMenuSnapshot {
        god_mode: state.god_mode,
        noclip: state.noclip,
        arming_teleport: state.arming_teleport,
        level,
    };
    if !root_ref.is_changed() && last.as_ref() == Some(&want) {
        return;
    }
    let (Some(palette), Some(theme)) = (palette.as_deref(), theme.as_deref()) else {
        return;
    };
    let Ok(body) = content.single() else {
        return;
    };
    *last = Some(want);

    commands.entity(body).despawn_related::<Children>();
    commands.entity(body).with_children(|root| {
        // Toggles read as Primary (call-to-action gold) when ON, Secondary
        // (default dark) when OFF, so the active state is obvious at a glance.
        spawn_row_button(
            root,
            theme,
            palette,
            &format!("God Mode: {}", on_off(state.god_mode)),
            DebugMenuButton::ToggleGodMode,
            toggle_style(state.god_mode),
        );
        spawn_row_button(
            root,
            theme,
            palette,
            &format!("Noclip: {}", on_off(state.noclip)),
            DebugMenuButton::ToggleNoclip,
            toggle_style(state.noclip),
        );
        spawn_row_button(
            root,
            theme,
            palette,
            "Full Heal",
            DebugMenuButton::FullHeal,
            ButtonStyle::Secondary,
        );

        // Level row: [-] Level N [+]
        root.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(8.0),
            margin: UiRect::vertical(Val::Px(3.0)),
            ..default()
        })
        .with_children(|row| {
            spawn_step_button(row, theme, palette, "-", DebugMenuButton::LevelDown);
            row.spawn((
                Text::new(format!("Level {level}")),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(palette.text_primary),
                Node {
                    min_width: Val::Px(70.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ));
            spawn_step_button(row, theme, palette, "+", DebugMenuButton::LevelUp);
        });

        spawn_row_button(
            root,
            theme,
            palette,
            &format!("+{XP_GRANT_STEP} XP"),
            DebugMenuButton::GrantXp,
            ButtonStyle::Secondary,
        );
        let (teleport_label, teleport_style) = if state.arming_teleport {
            ("Teleport: click a tile…", ButtonStyle::Primary)
        } else {
            ("Teleport to cursor", ButtonStyle::Secondary)
        };
        spawn_row_button(
            root,
            theme,
            palette,
            teleport_label,
            DebugMenuButton::TeleportToCursor,
            teleport_style,
        );
    });
}

fn on_off(on: bool) -> &'static str {
    if on {
        "ON"
    } else {
        "OFF"
    }
}

fn toggle_style(on: bool) -> ButtonStyle {
    if on {
        ButtonStyle::Primary
    } else {
        ButtonStyle::Secondary
    }
}

/// A full-width themed button row (uses the shared `ThemedButton` widget so it
/// matches every other button in the project, with hover/press feedback).
fn spawn_row_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    button: DebugMenuButton,
    style: ButtonStyle,
) {
    let node = Node {
        width: Val::Percent(100.0),
        padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
        margin: UiRect::vertical(Val::Px(2.0)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    spawn_themed_button(parent, theme, palette, style, node, label, 14.0, button);
}

/// A small inline themed button (the level `-` / `+` steppers).
fn spawn_step_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    button: DebugMenuButton,
) {
    let node = Node {
        padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    spawn_themed_button(
        parent,
        theme,
        palette,
        ButtonStyle::Secondary,
        node,
        label,
        16.0,
        button,
    );
}

fn handle_debug_menu_clicks(
    mut pending: ResMut<PendingGameCommands>,
    mut state: ResMut<DebugMenuState>,
    client_state: Res<ClientGameState>,
    interactions: Query<(&Interaction, &DebugMenuButton), Changed<Interaction>>,
) {
    let level = client_state
        .experience
        .as_ref()
        .map(|e| e.level)
        .unwrap_or(1);
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            DebugMenuButton::ToggleGodMode => {
                state.god_mode = !state.god_mode;
                pending.push(GameCommand::AdminToggleGodMode);
            }
            DebugMenuButton::ToggleNoclip => {
                state.noclip = !state.noclip;
                pending.push(GameCommand::AdminToggleNoclip);
            }
            DebugMenuButton::FullHeal => {
                pending.push(GameCommand::AdminFullHeal);
            }
            DebugMenuButton::LevelUp => {
                let next = (level + 1).min(LEVEL_CAP);
                pending.push(GameCommand::AdminSetLevel { level: next });
            }
            DebugMenuButton::LevelDown => {
                let next = level.saturating_sub(1).max(1);
                pending.push(GameCommand::AdminSetLevel { level: next });
            }
            DebugMenuButton::GrantXp => {
                pending.push(GameCommand::AdminGrantXp {
                    amount: XP_GRANT_STEP,
                });
            }
            DebugMenuButton::TeleportToCursor => {
                // Can't teleport now — the cursor is over the button, not a
                // world tile. Arm it; `handle_teleport_arming` fires on the
                // next world click.
                state.arming_teleport = !state.arming_teleport;
            }
        }
    }
}

/// While teleport is armed, the next left-click over a world tile teleports the
/// player there. `HoveredTile` is `Some` only when the cursor is over the world
/// (not UI), so clicking a panel button never triggers it.
fn handle_teleport_arming(
    mut state: ResMut<DebugMenuState>,
    mouse: Res<ButtonInput<MouseButton>>,
    hovered: Res<HoveredTile>,
    mut pending: ResMut<PendingGameCommands>,
) {
    if !state.arming_teleport {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        if let Some(tile) = hovered.0 {
            pending.push(GameCommand::AdminTeleport {
                space_id: None,
                tile_position: tile,
            });
            state.arming_teleport = false;
        }
    }
}
