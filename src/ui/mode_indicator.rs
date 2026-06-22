//! Top-right HUD **mode indicator** box: one icon cell per toggle-mode
//! (Sneaking, Aware). A cell lights up while its mode is active, shows a tooltip
//! on hover (name + bonus), and toggles the mode when clicked. Icons live in
//! `assets/ui/hud_indicators/{sneak,aware}.png`.
//!
//! Purely presentation: cells read the replicated `ClientGameState.{sneaking,
//! aware}` flags and dispatch `SetSneaking` / `SetAware` commands on click — the
//! same flags/commands the keybinds (`V` / `F`) use.

use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::game::commands::GameCommand;
use crate::game::resources::{ClientGameState, PendingGameCommands};
use crate::ui::components::HudRoot;
use crate::ui::menu_bar::MENU_BAR_HEIGHT;
use crate::ui::theme::palette::Palette;

/// Distance of the box's right edge from the screen's right edge — sits just
/// left of the time-of-day button (which is at `right: 348` + 48 wide).
const BOX_RIGHT: f32 = 402.0;
const CELL_SIZE: f32 = 34.0;
const ICON_SIZE: f32 = 26.0;

const BOX_BG: Color = Color::srgba(0.10, 0.08, 0.04, 0.92);
const BOX_BORDER: Color = Color::srgb(0.60, 0.45, 0.24);
const CELL_BG_ON: Color = Color::srgba(0.32, 0.27, 0.12, 0.95);
const CELL_BG_OFF: Color = Color::srgba(0.06, 0.05, 0.04, 0.55);
const CELL_BORDER_ON: Color = Color::srgb(0.95, 0.78, 0.36);
const CELL_BORDER_OFF: Color = Color::srgb(0.34, 0.29, 0.20);
const ICON_TINT_ON: Color = Color::WHITE;
const ICON_TINT_OFF: Color = Color::srgba(0.55, 0.55, 0.55, 0.55);

/// The toggle-modes shown in the box, in display order.
const MODES: [HudMode; 2] = [HudMode::Sneak, HudMode::Aware];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudMode {
    Sneak,
    Aware,
}

impl HudMode {
    fn icon_path(self) -> &'static str {
        match self {
            HudMode::Sneak => "ui/hud_indicators/sneak.png",
            HudMode::Aware => "ui/hud_indicators/aware.png",
        }
    }

    fn is_active(self, state: &ClientGameState) -> bool {
        match self {
            HudMode::Sneak => state.sneaking,
            HudMode::Aware => state.aware,
        }
    }

    /// The command that flips this mode from its current replicated state.
    fn toggle_command(self, state: &ClientGameState) -> GameCommand {
        match self {
            HudMode::Sneak => GameCommand::SetSneaking {
                sneaking: !state.sneaking,
            },
            HudMode::Aware => GameCommand::SetAware {
                aware: !state.aware,
            },
        }
    }

    /// Hover-tooltip text: name + keybind on the first line, effect on the next.
    fn tooltip(self) -> &'static str {
        match self {
            HudMode::Sneak => "Sneaking (V)\nSlower & quieter; roll Stealth vs detection.",
            HudMode::Aware => "Aware (F)\nSlower; +5 to spot hidden things & read foes.",
        }
    }
}

#[derive(Component)]
struct ModeIndicatorBox;

#[derive(Component)]
struct ModeToggleButton {
    mode: HudMode,
}

#[derive(Component)]
struct ModeToggleIcon {
    mode: HudMode,
}

#[derive(Component)]
struct ModeTooltipRoot;

#[derive(Component)]
struct ModeTooltipLabel;

/// Register the mode-indicator systems (sync highlight, click-to-toggle, hover
/// tooltip). The box itself is spawned by `spawn_mode_indicator_box` from
/// `spawn_hud`.
pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        (
            sync_mode_indicators,
            handle_mode_toggle_clicks,
            sync_mode_tooltip,
        )
            .run_if(in_state(ClientAppState::InGame)),
    );
}

/// Spawn the box + its cells and the (hidden) hover tooltip. Called from
/// `spawn_hud` alongside the character / time-of-day buttons.
pub fn spawn_mode_indicator_box(
    commands: &mut Commands,
    asset_server: &AssetServer,
    palette: &Palette,
) {
    commands
        .spawn((
            ModeIndicatorBox,
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(MENU_BAR_HEIGHT + 12.0),
                right: Val::Px(BOX_RIGHT),
                height: Val::Px(48.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(6.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(BOX_BG),
            BorderColor::all(BOX_BORDER),
            GlobalZIndex(50),
        ))
        .with_children(|box_node| {
            for mode in MODES {
                spawn_mode_cell(box_node, asset_server, mode);
            }
        });

    // Hover tooltip, dropped just below the box. Hidden until a cell is hovered.
    commands
        .spawn((
            ModeTooltipRoot,
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(MENU_BAR_HEIGHT + 12.0 + 48.0 + 4.0),
                right: Val::Px(BOX_RIGHT),
                max_width: Val::Px(220.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(60),
            BackgroundColor(BOX_BG),
            BorderColor::all(palette.border_accent),
        ))
        .with_children(|tooltip| {
            tooltip.spawn((
                Text::new(""),
                ModeTooltipLabel,
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(palette.text_primary),
            ));
        });
}

fn spawn_mode_cell(parent: &mut ChildSpawnerCommands, asset_server: &AssetServer, mode: HudMode) {
    let icon: Handle<Image> = asset_server.load(mode.icon_path());
    parent
        .spawn((
            Button,
            ModeToggleButton { mode },
            Node {
                width: Val::Px(CELL_SIZE),
                height: Val::Px(CELL_SIZE),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(CELL_BG_OFF),
            BorderColor::all(CELL_BORDER_OFF),
        ))
        .with_children(|cell| {
            cell.spawn((
                Node {
                    width: Val::Px(ICON_SIZE),
                    height: Val::Px(ICON_SIZE),
                    ..default()
                },
                ImageNode::new(icon).with_color(ICON_TINT_OFF),
                ModeToggleIcon { mode },
            ));
        });
}

/// Light up each cell (background, border, icon tint) based on whether its mode
/// is currently active in `ClientGameState`.
fn sync_mode_indicators(
    client_state: Res<ClientGameState>,
    mut buttons: Query<(&ModeToggleButton, &mut BackgroundColor, &mut BorderColor)>,
    mut icons: Query<(&ModeToggleIcon, &mut ImageNode)>,
) {
    for (button, mut bg, mut border) in &mut buttons {
        let active = button.mode.is_active(&client_state);
        let bg_color = if active { CELL_BG_ON } else { CELL_BG_OFF };
        let border_color = if active {
            CELL_BORDER_ON
        } else {
            CELL_BORDER_OFF
        };
        if bg.0 != bg_color {
            bg.0 = bg_color;
        }
        let next_border = BorderColor::all(border_color);
        if *border != next_border {
            *border = next_border;
        }
    }
    for (icon, mut image) in &mut icons {
        let tint = if icon.mode.is_active(&client_state) {
            ICON_TINT_ON
        } else {
            ICON_TINT_OFF
        };
        if image.color != tint {
            image.color = tint;
        }
    }
}

/// Clicking a cell toggles its mode by pushing the matching `Set*` command —
/// the same command the keybind uses, so the round-trip through the server is
/// identical.
fn handle_mode_toggle_clicks(
    interactions: Query<(&Interaction, &ModeToggleButton), Changed<Interaction>>,
    client_state: Res<ClientGameState>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    for (interaction, button) in &interactions {
        if matches!(interaction, Interaction::Pressed) {
            pending_commands.push(button.mode.toggle_command(&client_state));
        }
    }
}

/// Show the tooltip with the hovered cell's name + bonus; hide it otherwise.
fn sync_mode_tooltip(
    buttons: Query<(&Interaction, &ModeToggleButton)>,
    mut root: Query<&mut Visibility, With<ModeTooltipRoot>>,
    mut label: Query<&mut Text, With<ModeTooltipLabel>>,
) {
    let Ok(mut visibility) = root.single_mut() else {
        return;
    };
    let Ok(mut text) = label.single_mut() else {
        return;
    };
    let hovered = buttons
        .iter()
        .find(|(interaction, _)| matches!(interaction, Interaction::Hovered | Interaction::Pressed))
        .map(|(_, button)| button.mode);
    match hovered {
        Some(mode) => {
            *visibility = Visibility::Visible;
            let tip = mode.tooltip();
            if text.0 != tip {
                text.0 = tip.to_owned();
            }
        }
        None => {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
        }
    }
}
