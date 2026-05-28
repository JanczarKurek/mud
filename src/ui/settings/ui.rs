//! The settings overlay: a full-screen modal spawned once at `Startup` and
//! toggled via [`SettingsUiState`]. Reused identically on the title screen
//! and over the in-game HUD — it owns its own root entity (not tagged
//! `HudRoot`/`TitleScreenRoot`) so it survives every state transition.

use bevy::ecs::message::{MessageReader, Messages};
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input::ButtonInput;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton};
use crate::ui::theme::{Palette, UiThemeAssets};

use super::display::{DisplayOption, DisplaySettings};
use super::gameplay::{GameplayOption, GameplaySettings};
use super::keycode_serde::is_modifier_key;
use super::model::{all_actions, Action, Binding, Keybindings, Modifiers, MovementDir};

/// Which section of the settings modal is shown. The tab bar is array-driven
/// off [`SettingsSection::ALL`] so adding a section is one enum variant plus
/// its rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsSection {
    #[default]
    Controls,
    Display,
    Gameplay,
}

impl SettingsSection {
    const ALL: [SettingsSection; 3] = [Self::Controls, Self::Display, Self::Gameplay];

    fn label(self) -> &'static str {
        match self {
            Self::Controls => "Controls",
            Self::Display => "Display",
            Self::Gameplay => "Gameplay",
        }
    }
}

/// What the next captured keypress should rebind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureTarget {
    Action(Action),
    Movement(MovementDir),
}

#[derive(Resource, Default)]
pub struct SettingsUiState {
    pub open: bool,
    pub section: SettingsSection,
    pub capturing: Option<CaptureTarget>,
    /// Transient "(was: X)" hint shown on the row that just displaced a
    /// binding. Cleared when a new capture starts.
    note: Option<(CaptureTarget, String)>,
}

impl SettingsUiState {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.capturing = None;
        }
    }
}

/// Ordering anchor: capture reads the raw key events before the global
/// input-swallow blanks them.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SettingsCaptureSet;

#[derive(Component)]
pub struct SettingsOverlayRoot;

#[derive(Component)]
pub struct SettingsScrollList;

/// A clickable tab in the section bar, plus its text child (recolored on
/// selection).
#[derive(Component, Clone, Copy)]
pub struct SettingsTabButton(SettingsSection);

#[derive(Component, Clone, Copy)]
pub struct SettingsTabLabel(SettingsSection);

/// Wraps every row of one section so visibility is a single `Node.display`
/// toggle per section rather than per row.
#[derive(Component, Clone, Copy)]
pub struct SectionContent(SettingsSection);

/// A Display-section row whose button cycles one [`DisplayOption`].
#[derive(Component, Clone, Copy)]
pub struct OptionRowButton(DisplayOption);

#[derive(Component, Clone, Copy)]
pub struct OptionRowLabel(DisplayOption);

/// A Gameplay-section row whose button cycles one [`GameplayOption`].
#[derive(Component, Clone, Copy)]
pub struct GameplayOptionRowButton(GameplayOption);

#[derive(Component, Clone, Copy)]
pub struct GameplayOptionRowLabel(GameplayOption);

#[derive(Component, Clone, Copy)]
pub struct BindingRowButton {
    target: CaptureTarget,
}

#[derive(Component, Clone, Copy)]
pub struct BindingRowLabel {
    target: CaptureTarget,
}

#[derive(Component)]
pub struct SettingsCloseButton;

#[derive(Component)]
pub struct SettingsResetButton;

pub fn is_capturing(state: Res<SettingsUiState>) -> bool {
    state.capturing.is_some()
}

pub fn is_open(state: Res<SettingsUiState>) -> bool {
    state.open
}

/// Build the (hidden) overlay tree once. Theme/palette are inserted at
/// plugin-build time so they're available this early.
pub fn spawn_settings_overlay(
    mut commands: Commands,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
) {
    let theme = theme.clone();
    let palette = *palette;
    let (btn_bg, btn_border, btn_text) = idle_colors(&palette, ButtonStyle::Secondary, false);

    commands
        .spawn((
            SettingsOverlayRoot,
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            // Opaque-ish backdrop blocks click-through to the HUD / title
            // screen behind it.
            BackgroundColor(Color::srgba(0.02, 0.02, 0.03, 0.82)),
            GlobalZIndex(i32::MAX - 10),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(620.0),
                    max_width: percent(94.0),
                    max_height: percent(85.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(12.0),
                    padding: UiRect::all(px(20.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                ImageNode::new(theme.panel_frame.clone())
                    .with_mode(theme.panel_image_mode())
                    .with_color(Color::WHITE),
                BackgroundColor(Color::NONE),
                BorderColor::all(palette.border_accent),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Settings"),
                    TextFont {
                        font_size: 30.0,
                        ..default()
                    },
                    TextColor(palette.text_primary),
                ));

                // Section tab bar, driven off `SettingsSection::ALL`.
                panel
                    .spawn((Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(8.0),
                        ..default()
                    },))
                    .with_children(|tabs| {
                        for section in SettingsSection::ALL {
                            spawn_tab(tabs, &palette, section);
                        }
                    });

                // Scrollable list of rows — flex_grow so it eats the panel's
                // remaining height and every row is reachable by scrolling.
                panel
                    .spawn((
                        SettingsScrollList,
                        Node {
                            width: percent(100.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4.0),
                            flex_grow: 1.0,
                            min_height: px(0.0),
                            padding: UiRect::all(px(6.0)),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        ScrollPosition::default(),
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                    ))
                    .with_children(|list| {
                        // Controls section (shown by default).
                        list.spawn((
                            SectionContent(SettingsSection::Controls),
                            Node {
                                width: percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(4.0),
                                ..default()
                            },
                        ))
                        .with_children(|sec| {
                            spawn_section_header(sec, &palette, "Movement");
                            for dir in MovementDir::ALL {
                                spawn_row(
                                    sec,
                                    &theme,
                                    &palette,
                                    dir.label(),
                                    CaptureTarget::Movement(dir),
                                    btn_bg,
                                    btn_border,
                                    btn_text,
                                );
                            }
                            spawn_section_header(sec, &palette, "Actions");
                            for action in all_actions() {
                                spawn_row(
                                    sec,
                                    &theme,
                                    &palette,
                                    &action.label(),
                                    CaptureTarget::Action(action),
                                    btn_bg,
                                    btn_border,
                                    btn_text,
                                );
                            }
                        });

                        // Display section (hidden until its tab is selected).
                        list.spawn((
                            SectionContent(SettingsSection::Display),
                            Node {
                                width: percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(4.0),
                                display: Display::None,
                                ..default()
                            },
                        ))
                        .with_children(|sec| {
                            spawn_section_header(sec, &palette, "Display");
                            for opt in DisplayOption::ALL {
                                spawn_option_row(
                                    sec, &theme, &palette, opt, btn_bg, btn_border, btn_text,
                                );
                            }
                        });

                        // Gameplay section (hidden until its tab is selected).
                        list.spawn((
                            SectionContent(SettingsSection::Gameplay),
                            Node {
                                width: percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(4.0),
                                display: Display::None,
                                ..default()
                            },
                        ))
                        .with_children(|sec| {
                            spawn_section_header(sec, &palette, "Gameplay");
                            for opt in GameplayOption::ALL {
                                spawn_gameplay_option_row(
                                    sec, &theme, &palette, opt, btn_bg, btn_border, btn_text,
                                );
                            }
                        });
                    });

                // Sticky footer (outside the scroll area).
                panel
                    .spawn((Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: px(12.0),
                        ..default()
                    },))
                    .with_children(|footer| {
                        spawn_footer_button(
                            footer,
                            &theme,
                            &palette,
                            ButtonStyle::Danger,
                            "Reset to defaults",
                            SettingsResetButton,
                        );
                        spawn_footer_button(
                            footer,
                            &theme,
                            &palette,
                            ButtonStyle::Primary,
                            "Close",
                            SettingsCloseButton,
                        );
                    });
            });
        });
}

fn spawn_tab(parent: &mut ChildSpawnerCommands, palette: &Palette, section: SettingsSection) {
    let selected = section == SettingsSection::default();
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Ghost, selected);
    parent
        .spawn((
            Button,
            ThemedButton {
                style: ButtonStyle::Ghost,
                selected,
            },
            SettingsTabButton(section),
            Node {
                padding: UiRect::axes(px(14.0), px(6.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor::all(border),
        ))
        .with_children(|tab| {
            tab.spawn((
                SettingsTabLabel(section),
                Text::new(section.label()),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_option_row(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    opt: DisplayOption,
    btn_bg: Color,
    btn_border: Color,
    btn_text: Color,
) {
    parent
        .spawn((Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: px(12.0),
            padding: UiRect::axes(px(8.0), px(4.0)),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Text::new(opt.label()),
                TextFont {
                    font_size: 15.0,
                    ..default()
                },
                TextColor(palette.text_primary),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            row.spawn((
                Button,
                ThemedButton::new(ButtonStyle::Secondary),
                OptionRowButton(opt),
                Node {
                    width: px(190.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(10.0), px(6.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                ImageNode::new(theme.button_frame.clone())
                    .with_mode(theme.button_image_mode())
                    .with_color(btn_bg),
                BackgroundColor(Color::NONE),
                BorderColor::all(btn_border),
            ))
            .with_children(|button| {
                button.spawn((
                    OptionRowLabel(opt),
                    Text::new(""),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(btn_text),
                ));
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_gameplay_option_row(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    opt: GameplayOption,
    btn_bg: Color,
    btn_border: Color,
    btn_text: Color,
) {
    parent
        .spawn((Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: px(12.0),
            padding: UiRect::axes(px(8.0), px(4.0)),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Text::new(opt.label()),
                TextFont {
                    font_size: 15.0,
                    ..default()
                },
                TextColor(palette.text_primary),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            row.spawn((
                Button,
                ThemedButton::new(ButtonStyle::Secondary),
                GameplayOptionRowButton(opt),
                Node {
                    width: px(190.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(10.0), px(6.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                ImageNode::new(theme.button_frame.clone())
                    .with_mode(theme.button_image_mode())
                    .with_color(btn_bg),
                BackgroundColor(Color::NONE),
                BorderColor::all(btn_border),
            ))
            .with_children(|button| {
                button.spawn((
                    GameplayOptionRowLabel(opt),
                    Text::new(""),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(btn_text),
                ));
            });
        });
}

fn spawn_section_header(parent: &mut ChildSpawnerCommands, palette: &Palette, label: &str) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(palette.text_accent),
        Node {
            margin: UiRect::top(px(8.0)),
            ..default()
        },
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_row(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    target: CaptureTarget,
    btn_bg: Color,
    btn_border: Color,
    btn_text: Color,
) {
    parent
        .spawn((Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: px(12.0),
            padding: UiRect::axes(px(8.0), px(4.0)),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: 15.0,
                    ..default()
                },
                TextColor(palette.text_primary),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            row.spawn((
                Button,
                ThemedButton::new(ButtonStyle::Secondary),
                BindingRowButton { target },
                Node {
                    width: px(190.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(10.0), px(6.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                ImageNode::new(theme.button_frame.clone())
                    .with_mode(theme.button_image_mode())
                    .with_color(btn_bg),
                BackgroundColor(Color::NONE),
                BorderColor::all(btn_border),
            ))
            .with_children(|button| {
                button.spawn((
                    BindingRowLabel { target },
                    Text::new(""),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(btn_text),
                ));
            });
        });
}

fn spawn_footer_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    style: ButtonStyle,
    label: &str,
    marker: M,
) {
    let (bg, border, text) = idle_colors(palette, style, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(style),
            marker,
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20.0), px(12.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            ImageNode::new(theme.button_frame.clone())
                .with_mode(theme.button_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

/// Mirror `SettingsUiState.open` onto the overlay root's `Display`.
pub fn sync_settings_overlay_visibility(
    state: Res<SettingsUiState>,
    mut root: Query<&mut Node, With<SettingsOverlayRoot>>,
) {
    let want = if state.open {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut root {
        if node.display != want {
            node.display = want;
        }
    }
}

/// Repaint every row button: the action/direction's current binding, the
/// capture prompt while it's being rebound, plus the transient eviction hint.
pub fn sync_binding_row_labels(
    state: Res<SettingsUiState>,
    keybindings: Res<Keybindings>,
    mut labels: Query<(&BindingRowLabel, &mut Text)>,
) {
    for (label, mut text) in &mut labels {
        let desired = if state.capturing == Some(label.target) {
            "Press a key… (Esc to cancel)".to_owned()
        } else {
            let mut base = match label.target {
                CaptureTarget::Action(action) => keybindings.bindings(action).display(),
                CaptureTarget::Movement(dir) => keybindings.movement.display(dir),
            };
            if let Some((note_target, note)) = &state.note {
                if *note_target == label.target {
                    base.push_str(&format!("  (was: {note})"));
                }
            }
            base
        };
        if text.0 != desired {
            text.0 = desired;
        }
    }
}

/// Click a row → arm capture for that target.
pub fn handle_binding_row_clicks(
    mut state: ResMut<SettingsUiState>,
    buttons: Query<(&Interaction, &BindingRowButton), Changed<Interaction>>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            state.capturing = Some(button.target);
            state.note = None;
        }
    }
}

/// PreUpdate, runs only while capturing and *before* the global swallow and
/// the terminal/chat/console consumers — so rebinding to backtick/T works
/// and the captured key never leaks into a focused terminal.
pub fn capture_keybind(
    mut key_events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<SettingsUiState>,
    mut keybindings: ResMut<Keybindings>,
) {
    let Some(target) = state.capturing else {
        return;
    };

    for event in key_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        let key = event.key_code;

        if key == KeyCode::Escape {
            state.capturing = None;
            return;
        }
        // Wait for the real key so the user can hold Ctrl then press it.
        if is_modifier_key(key) {
            continue;
        }

        let evicted = match target {
            CaptureTarget::Action(action) => {
                let mods = Modifiers {
                    ctrl: keyboard.pressed(KeyCode::ControlLeft)
                        || keyboard.pressed(KeyCode::ControlRight),
                    shift: keyboard.pressed(KeyCode::ShiftLeft)
                        || keyboard.pressed(KeyCode::ShiftRight),
                    alt: keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight),
                };
                keybindings.rebind_action(action, Binding::new(key, mods))
            }
            // Movement is always a plain (no-modifier) key.
            CaptureTarget::Movement(dir) => keybindings.rebind_movement(dir, key),
        };

        state.note = evicted.map(|label| (target, label));
        state.capturing = None;
        return;
    }
}

/// The single global choke point: while the modal is open, blank the
/// keyboard so **no** gameplay/terminal system sees input — no per-system
/// run conditions needed.
pub fn swallow_input_while_settings_open(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut key_events: ResMut<Messages<KeyboardInput>>,
) {
    keyboard.reset_all();
    key_events.clear();
}

pub fn handle_settings_close_button(
    mut state: ResMut<SettingsUiState>,
    buttons: Query<&Interaction, (With<SettingsCloseButton>, Changed<Interaction>)>,
) {
    for interaction in &buttons {
        if *interaction == Interaction::Pressed {
            state.open = false;
            state.capturing = None;
        }
    }
}

pub fn handle_settings_reset_button(
    mut keybindings: ResMut<Keybindings>,
    mut state: ResMut<SettingsUiState>,
    buttons: Query<&Interaction, (With<SettingsResetButton>, Changed<Interaction>)>,
) {
    for interaction in &buttons {
        if *interaction == Interaction::Pressed {
            keybindings.reset_to_defaults();
            state.capturing = None;
            state.note = None;
        }
    }
}

/// Click a tab → switch the visible section.
pub fn handle_tab_clicks(
    mut state: ResMut<SettingsUiState>,
    buttons: Query<(&Interaction, &SettingsTabButton), Changed<Interaction>>,
) {
    for (interaction, tab) in &buttons {
        if *interaction == Interaction::Pressed && state.section != tab.0 {
            state.section = tab.0;
            state.capturing = None;
        }
    }
}

/// Mirror `state.section` onto section visibility and the tab bar's selected
/// styling. Tabs carry no `ImageNode`, so the global themed-button recolor
/// skips them — selection is repainted here instead.
pub fn sync_section_visibility(
    state: Res<SettingsUiState>,
    palette: Res<Palette>,
    mut sections: Query<(&SectionContent, &mut Node)>,
    mut tabs: Query<(
        &SettingsTabButton,
        &mut ThemedButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut tab_labels: Query<(&SettingsTabLabel, &mut TextColor)>,
) {
    if !state.is_changed() {
        return;
    }
    for (sec, mut node) in &mut sections {
        let want = if sec.0 == state.section {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
    for (tab, mut themed, mut bg, mut border) in &mut tabs {
        let selected = tab.0 == state.section;
        themed.selected = selected;
        let (b, bd, _t) = idle_colors(&palette, ButtonStyle::Ghost, selected);
        *bg = BackgroundColor(b);
        *border = BorderColor::all(bd);
    }
    for (label, mut color) in &mut tab_labels {
        let selected = label.0 == state.section;
        let (_b, _bd, t) = idle_colors(&palette, ButtonStyle::Ghost, selected);
        *color = TextColor(t);
    }
}

/// Repaint each Display-section row button with its option's current value.
pub fn sync_option_row_labels(
    display: Res<DisplaySettings>,
    mut labels: Query<(&OptionRowLabel, &mut Text)>,
) {
    for (label, mut text) in &mut labels {
        let desired = label.0.value_label(&display);
        if text.0 != desired {
            text.0 = desired;
        }
    }
}

/// Click a Display row → advance that option to its next value.
pub fn handle_option_row_clicks(
    mut display: ResMut<DisplaySettings>,
    buttons: Query<(&Interaction, &OptionRowButton), Changed<Interaction>>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            button.0.cycle(&mut display);
        }
    }
}

/// Repaint each Gameplay-section row button with its option's current value.
pub fn sync_gameplay_option_row_labels(
    gameplay: Res<GameplaySettings>,
    mut labels: Query<(&GameplayOptionRowLabel, &mut Text)>,
) {
    for (label, mut text) in &mut labels {
        let desired = label.0.value_label(&gameplay);
        if text.0 != desired {
            text.0 = desired;
        }
    }
}

/// Click a Gameplay row → advance that option to its next value.
pub fn handle_gameplay_option_row_clicks(
    mut gameplay: ResMut<GameplaySettings>,
    buttons: Query<(&Interaction, &GameplayOptionRowButton), Changed<Interaction>>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            button.0.cycle(&mut gameplay);
        }
    }
}

/// Mouse-wheel scroll for the controls list (only while open).
pub fn handle_settings_scroll(
    state: Res<SettingsUiState>,
    mut wheel: MessageReader<MouseWheel>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut list: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        With<SettingsScrollList>,
    >,
) {
    if !state.open {
        return;
    }
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    for ev in wheel.read() {
        let mut delta = -ev.y;
        if ev.unit == MouseScrollUnit::Line {
            delta *= 21.0;
        }
        if delta == 0.0 {
            continue;
        }
        for (computed, transform, mut scroll) in &mut list {
            if !crate::ui::trade::point_in_ui_node(cursor, computed, transform) {
                continue;
            }
            let max_offset =
                (computed.content_size() - computed.size()) * computed.inverse_scale_factor();
            if max_offset.y <= 0.0 {
                continue;
            }
            scroll.y = (scroll.y + delta).clamp(0.0, max_offset.y);
        }
    }
}
