use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};

use crate::app::state::{ClientAppState, DiagnosticPause, LocalSelectedCharacter};
use crate::diagnostics::{DebugAction, PendingDebugActions, PerfOverlayState};
use crate::game::resources::ClientGameState;
use crate::network::resources::{PendingPlayerSave, PendingPlayerSaves, TcpClientConnection};
use crate::player::components::{Player, PlayerIdentity};
use crate::ui::components::{
    CoordinateReadout, HudRoot, MenuBarItemButton, MenuBarRoot, MenuDropdownEntryButton,
    MenuDropdownRoot, ToggleSource,
};
use crate::ui::mountable_panel::PanelMountMode;
use crate::ui::resources::{
    DockedPanelState, HoveredTile, MenuAction, MenuBarId, MinimapPanelMode, OpenMenuState,
    PendingMenuActions, ShowCoordinates,
};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton, ThemedPanel};
use crate::ui::theme::{Palette, UiThemeAssets};

pub const MENU_BAR_HEIGHT: f32 = 26.0;

struct MenuDefinition {
    id: MenuBarId,
    label: &'static str,
    entries: &'static [MenuEntry],
}

struct MenuEntry {
    label: &'static str,
    action: MenuAction,
    /// When `Some`, the entry's first three characters are rewritten each
    /// frame with `[X]` / `[ ]` based on the referenced toggle.
    toggle: Option<ToggleSource>,
}

const fn entry(label: &'static str, action: MenuAction) -> MenuEntry {
    MenuEntry {
        label,
        action,
        toggle: None,
    }
}

const fn toggle_entry(label: &'static str, action: MenuAction, toggle: ToggleSource) -> MenuEntry {
    MenuEntry {
        label,
        action,
        toggle: Some(toggle),
    }
}

const MENU_DEFINITIONS: &[MenuDefinition] = &[
    MenuDefinition {
        id: MenuBarId::File,
        label: "File",
        entries: &[
            entry("Settings", MenuAction::OpenSettings),
            entry("Logout", MenuAction::Logout),
            entry("Quit", MenuAction::Quit),
        ],
    },
    MenuDefinition {
        id: MenuBarId::View,
        label: "View",
        entries: &[
            entry("Full Map  (M)", MenuAction::ToggleFullMap),
            entry("Minimap", MenuAction::ToggleMinimap),
            entry("Inventory", MenuAction::ToggleBackpack),
            entry("Character", MenuAction::ToggleStatus),
            entry("Equipment", MenuAction::ToggleEquipment),
            entry("Nearby NPCs", MenuAction::ToggleNearbyNpcs),
            entry("Log  (L)", MenuAction::ToggleLog),
        ],
    },
    MenuDefinition {
        id: MenuBarId::Window,
        label: "Window",
        entries: &[],
    },
    MenuDefinition {
        id: MenuBarId::Help,
        label: "Help",
        entries: &[],
    },
    MenuDefinition {
        id: MenuBarId::Debug,
        label: "Debug",
        entries: &[
            toggle_entry(
                "[ ] Grid           F2",
                MenuAction::ToggleGrid,
                ToggleSource::Grid,
            ),
            toggle_entry(
                "[ ] FPS            F3",
                MenuAction::ToggleFpsCompact,
                ToggleSource::FpsCompact,
            ),
            toggle_entry(
                "[ ] FPS Expanded   F4",
                MenuAction::ToggleFpsExpanded,
                ToggleSource::FpsExpanded,
            ),
            toggle_entry(
                "[ ] Pause Sim      F8",
                MenuAction::TogglePauseSim,
                ToggleSource::PauseSim,
            ),
            toggle_entry(
                "[ ] Hide Floor     F10",
                MenuAction::ToggleHideFloor,
                ToggleSource::HideFloor,
            ),
            toggle_entry(
                "[ ] Hide Darkness  F11",
                MenuAction::ToggleHideDarkness,
                ToggleSource::HideDarkness,
            ),
            toggle_entry(
                "[ ] Hide Objects   F12",
                MenuAction::ToggleHideObjects,
                ToggleSource::HideObjects,
            ),
            toggle_entry(
                "[ ] Show Coords",
                MenuAction::ToggleShowCoords,
                ToggleSource::ShowCoords,
            ),
            entry("    Log Snapshot   F5", MenuAction::LogSnapshot),
            entry("    Cycle Vsync    F6", MenuAction::CycleVsync),
        ],
    },
];

pub fn spawn_menu_bar(commands: &mut Commands, theme: &UiThemeAssets, palette: &Palette) {
    let (ghost_bg, _, ghost_text) = idle_colors(palette, ButtonStyle::Ghost, false);

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Px(MENU_BAR_HEIGHT),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            MenuBarRoot,
            HudRoot,
            ImageNode::new(theme.title_bar.clone())
                .with_mode(theme.title_bar_image_mode())
                .with_color(palette.surface_title_bar),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_muted),
            GlobalZIndex(i32::MAX - 20),
        ))
        .with_children(|bar| {
            for definition in MENU_DEFINITIONS {
                bar.spawn((
                    Button,
                    ThemedButton::new(ButtonStyle::Ghost),
                    MenuBarItemButton {
                        menu: definition.id,
                    },
                    Node {
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(ghost_bg),
                ))
                .with_children(|item| {
                    item.spawn((
                        Text::new(definition.label),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(ghost_text),
                    ));
                });
            }

            // Flex spacer pushes the coordinate readout to the right edge.
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });

            bar.spawn((
                CoordinateReadout,
                Text::new(""),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(ghost_text),
                Node {
                    margin: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
                    align_self: AlignSelf::Center,
                    ..default()
                },
                Visibility::Hidden,
            ));
        });

    for definition in MENU_DEFINITIONS {
        if definition.entries.is_empty() {
            continue;
        }
        commands
            .spawn((
                ThemedPanel,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(MENU_BAR_HEIGHT),
                    left: Val::Px(-999.0),
                    min_width: Val::Px(160.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    display: Display::None,
                    ..default()
                },
                MenuDropdownRoot {
                    menu: definition.id,
                },
                HudRoot,
                ImageNode::new(theme.panel_frame.clone())
                    .with_mode(theme.panel_image_mode())
                    .with_color(Color::WHITE),
                BackgroundColor(Color::NONE),
                BorderColor::all(palette.border_muted),
                GlobalZIndex(i32::MAX - 19),
            ))
            .with_children(|dropdown| {
                for menu_entry in definition.entries {
                    dropdown
                        .spawn((
                            Button,
                            ThemedButton::new(ButtonStyle::Ghost),
                            MenuDropdownEntryButton {
                                action: menu_entry.action,
                                toggle_indicator: menu_entry.toggle,
                            },
                            Node {
                                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                                width: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(ghost_bg),
                        ))
                        .with_children(|entry| {
                            entry.spawn((
                                Text::new(menu_entry.label),
                                TextFont {
                                    font_size: 14.0,
                                    ..default()
                                },
                                TextColor(ghost_text),
                            ));
                        });
                }
            });
    }
}

/// Routes menu-bar and dropdown clicks. Hover/press recoloring is handled by
/// the shared `apply_themed_button_tint` system, but we still need to mark
/// the currently-open top-level menu so it stays highlighted while its
/// dropdown is visible — we do that by toggling `ThemedButton.selected`.
pub fn handle_menu_bar_clicks(
    mut open_menu: ResMut<OpenMenuState>,
    mut pending: ResMut<PendingMenuActions>,
    mut items: Query<
        (&Interaction, &MenuBarItemButton, &mut ThemedButton),
        Without<MenuDropdownEntryButton>,
    >,
    entries: Query<(&Interaction, &MenuDropdownEntryButton), Without<MenuBarItemButton>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let mut any_hovered = false;

    for (interaction, button, mut themed) in &mut items {
        let hovered = *interaction == Interaction::Hovered || *interaction == Interaction::Pressed;
        if hovered {
            any_hovered = true;
        }
        let active = open_menu.open_id == Some(button.menu);
        themed.selected = active;
        if *interaction == Interaction::Pressed && mouse.just_pressed(MouseButton::Left) {
            if open_menu.open_id == Some(button.menu) {
                open_menu.open_id = None;
            } else {
                open_menu.open_id = Some(button.menu);
            }
        }
    }

    for (interaction, entry) in &entries {
        let hovered = *interaction == Interaction::Hovered || *interaction == Interaction::Pressed;
        if hovered {
            any_hovered = true;
        }
        if *interaction == Interaction::Pressed && mouse.just_pressed(MouseButton::Left) {
            pending.actions.push(entry.action);
            open_menu.open_id = None;
        }
    }

    if keys.just_pressed(KeyCode::Escape) && open_menu.open_id.is_some() {
        open_menu.open_id = None;
    }

    if mouse.just_pressed(MouseButton::Left) && open_menu.open_id.is_some() && !any_hovered {
        open_menu.open_id = None;
    }
}

pub fn sync_menu_dropdowns(
    open_menu: Res<OpenMenuState>,
    item_query: Query<(&MenuBarItemButton, &ComputedNode, &UiGlobalTransform)>,
    mut dropdowns: Query<(&MenuDropdownRoot, &mut Node)>,
) {
    for (dropdown, mut node) in &mut dropdowns {
        let open = open_menu.open_id == Some(dropdown.menu);
        node.display = if open { Display::Flex } else { Display::None };
        if !open {
            continue;
        }
        for (item, computed, transform) in item_query.iter() {
            if item.menu != dropdown.menu {
                continue;
            }
            // `transform.translation` and `computed.size()` are in physical
            // pixels, but `Val::Px` is interpreted as logical pixels — on
            // HiDPI (scale_factor = 2.0) leaving the conversion off pushes
            // every dropdown twice as far right as the button it anchors to.
            let inv = computed.inverse_scale_factor();
            let center_x = transform.translation.x * inv;
            let width = computed.size().x * inv;
            let left = center_x - width * 0.5;
            node.left = Val::Px(left);
            node.top = Val::Px(MENU_BAR_HEIGHT);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn apply_menu_actions(
    mut commands: Commands,
    mut pending: ResMut<PendingMenuActions>,
    mut minimap_mode: ResMut<MinimapPanelMode>,
    mut panel_state: ResMut<DockedPanelState>,
    mut app_exit: MessageWriter<AppExit>,
    mut next_state: ResMut<NextState<ClientAppState>>,
    mut connection: Option<ResMut<TcpClientConnection>>,
    mut pending_saves: Option<ResMut<PendingPlayerSaves>>,
    local_players: Query<(Entity, &PlayerIdentity), With<Player>>,
    theme: Option<Res<UiThemeAssets>>,
    palette: Option<Res<Palette>>,
    movable_windows: Query<(Entity, &crate::ui::movable_window::MovableWindow)>,
    mut settings_ui: ResMut<crate::ui::settings::SettingsUiState>,
    mut pending_debug: ResMut<PendingDebugActions>,
    mut show_coords: ResMut<ShowCoordinates>,
) {
    for action in pending.actions.drain(..) {
        match action {
            MenuAction::ToggleFullMap => {
                // Mirror the M-key handler: ensure the minimap panel is
                // open in the dock, then toggle it between Mounted and
                // Floating.
                use crate::ui::minimap_panel::MinimapPanel;
                use crate::ui::mountable_panel::MountablePanel;
                let panel_id = MinimapPanel::panel_id_for(());
                if !panel_state.is_open(panel_id) {
                    if let Some(def) = MinimapPanel::docked_definition(()) {
                        panel_state.panels.push(def);
                    }
                }
                minimap_mode.0 = match minimap_mode.0 {
                    PanelMountMode::Mounted => PanelMountMode::Floating {
                        last_position: MinimapPanel::floating_position(()),
                    },
                    PanelMountMode::Floating { .. } => PanelMountMode::Mounted,
                };
            }
            MenuAction::ToggleStatus => {
                toggle_panel::<crate::ui::status_panel::StatusPanel>(&mut panel_state);
            }
            MenuAction::ToggleBackpack => {
                toggle_panel::<crate::ui::backpack_panel::BackpackPanel>(&mut panel_state);
            }
            MenuAction::ToggleEquipment => {
                toggle_panel::<crate::ui::equipment_panel::EquipmentPanel>(&mut panel_state);
            }
            MenuAction::ToggleMinimap => {
                toggle_panel::<crate::ui::minimap_panel::MinimapPanel>(&mut panel_state);
            }
            MenuAction::ToggleNearbyNpcs => {
                toggle_panel::<crate::ui::nearby_npcs_panel::NearbyNpcsPanel>(&mut panel_state);
            }
            MenuAction::ToggleLog => {
                crate::ui::log_panel::toggle_log_window(
                    &mut commands,
                    theme.as_deref(),
                    palette.as_deref(),
                    &movable_windows,
                );
            }
            MenuAction::OpenSettings => {
                settings_ui.toggle();
            }
            MenuAction::Logout => {
                do_logout(
                    &mut commands,
                    &mut next_state,
                    connection.as_deref_mut(),
                    pending_saves.as_deref_mut(),
                    &local_players,
                );
            }
            MenuAction::Quit => {
                app_exit.write(AppExit::Success);
            }
            MenuAction::ToggleGrid => pending_debug.actions.push(DebugAction::ToggleGrid),
            MenuAction::ToggleFpsCompact => {
                pending_debug.actions.push(DebugAction::ToggleFpsCompact);
            }
            MenuAction::ToggleFpsExpanded => {
                pending_debug.actions.push(DebugAction::ToggleFpsExpanded);
            }
            MenuAction::TogglePauseSim => {
                pending_debug.actions.push(DebugAction::TogglePauseSim);
            }
            MenuAction::ToggleHideFloor => {
                pending_debug.actions.push(DebugAction::ToggleHideFloor);
            }
            MenuAction::ToggleHideDarkness => {
                pending_debug.actions.push(DebugAction::ToggleHideDarkness);
            }
            MenuAction::ToggleHideObjects => {
                pending_debug.actions.push(DebugAction::ToggleHideObjects);
            }
            MenuAction::LogSnapshot => pending_debug.actions.push(DebugAction::LogSnapshot),
            MenuAction::CycleVsync => pending_debug.actions.push(DebugAction::CycleVsync),
            MenuAction::ToggleShowCoords => {
                show_coords.0 = !show_coords.0;
            }
        }
    }
}

/// Rewrites the `[X]` / `[ ]` prefix on every dropdown entry tagged with a
/// `ToggleSource`, mirroring the underlying boolean state. Cheap: ~10 entries,
/// runs every frame but only mutates text when the value actually flipped.
pub fn sync_menu_toggle_labels(
    entries: Query<(&MenuDropdownEntryButton, &Children)>,
    mut texts: Query<&mut Text>,
    perf: Res<PerfOverlayState>,
    pause: Res<DiagnosticPause>,
    show_coords: Res<ShowCoordinates>,
) {
    for (entry, children) in &entries {
        let Some(source) = entry.toggle_indicator else {
            continue;
        };
        let on = match source {
            ToggleSource::Grid => perf.grid_visible,
            ToggleSource::FpsCompact => perf.compact_visible,
            ToggleSource::FpsExpanded => perf.expanded_visible,
            ToggleSource::PauseSim => pause.simulation,
            ToggleSource::HideFloor => perf.floor_hidden,
            ToggleSource::HideDarkness => perf.darkness_hidden,
            ToggleSource::HideObjects => perf.objects_hidden,
            ToggleSource::ShowCoords => show_coords.0,
        };
        let prefix = if on { "[X]" } else { "[ ]" };
        for &child in children {
            let Ok(mut text) = texts.get_mut(child) else {
                continue;
            };
            if text.0.len() < 3 {
                continue;
            }
            if &text.0[..3] != prefix {
                text.0.replace_range(..3, prefix);
            }
        }
    }
}

/// Updates the right-aligned coordinate readout on the menu bar. Hidden
/// entirely when `ShowCoordinates` is off so the menu bar stays clean for
/// non-debug sessions.
pub fn update_coordinate_readout(
    show_coords: Res<ShowCoordinates>,
    client_state: Res<ClientGameState>,
    hovered: Res<HoveredTile>,
    mut readout: Query<(&mut Text, &mut Visibility), With<CoordinateReadout>>,
) {
    let Ok((mut text, mut visibility)) = readout.single_mut() else {
        return;
    };
    if !show_coords.0 {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
        return;
    }
    if *visibility != Visibility::Inherited {
        *visibility = Visibility::Inherited;
    }
    let player = match client_state.player_tile_position {
        Some(p) => format!("You: {},{},{}", p.x, p.y, p.z),
        None => "You: -".to_string(),
    };
    let cursor = match hovered.0 {
        Some(p) => format!("Cursor: {},{},{}", p.x, p.y, p.z),
        None => "Cursor: -".to_string(),
    };
    let new = format!("{}    {}", player, cursor);
    if text.0 != new {
        text.0 = new;
    }
}

/// Tear down the active session and return to the title screen.
///
/// TcpClient mode: drops the socket; the server's `disconnect_peer` flushes
/// the player save. EmbeddedClient mode: queues a save via
/// `PendingPlayerSaves` (drained by `persist_disconnected_players` in the
/// `Last` schedule) — the same path used when a TCP peer disconnects.
fn do_logout(
    commands: &mut Commands,
    next_state: &mut NextState<ClientAppState>,
    connection: Option<&mut TcpClientConnection>,
    pending_saves: Option<&mut PendingPlayerSaves>,
    local_players: &Query<(Entity, &PlayerIdentity), With<Player>>,
) {
    if let Some(connection) = connection {
        connection.stream = None;
        connection.read_buffer.clear();
    }
    if let Some(pending_saves) = pending_saves {
        for (entity, identity) in local_players.iter() {
            pending_saves.entries.push(PendingPlayerSave {
                character_id: identity.id.0 as i64,
                player_entity: entity,
            });
        }
    }
    commands.insert_resource(ClientGameState::default());
    commands.insert_resource(LocalSelectedCharacter::default());
    next_state.set(ClientAppState::TitleScreen);
}

/// Generic toggle for any singleton [`MountablePanel`] — close if open,
/// otherwise push a fresh `DockedPanel` via the trait's
/// `docked_definition(())`. Each `MenuAction::Toggle*` call site just
/// supplies the panel type parameter.
///
/// [`MountablePanel`]: crate::ui::mountable_panel::MountablePanel
fn toggle_panel<P: crate::ui::mountable_panel::MountablePanel<Key = ()>>(
    panel_state: &mut DockedPanelState,
) {
    let panel_id = P::panel_id_for(());
    if panel_state.is_open(panel_id) {
        panel_state.close_panel(panel_id);
    } else if let Some(docked) = P::docked_definition(()) {
        panel_state.panels.push(docked);
    }
}
