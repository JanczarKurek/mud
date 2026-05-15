use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};

use crate::app::state::ClientAppState;
use crate::game::resources::ClientGameState;
use crate::network::resources::{PendingPlayerSave, PendingPlayerSaves, TcpClientConnection};
use crate::player::components::{Player, PlayerIdentity};
use crate::ui::components::{
    HudRoot, MenuBarItemButton, MenuBarRoot, MenuDropdownEntryButton, MenuDropdownRoot,
};
use crate::ui::resources::{
    DockedPanelState, FullMapWindowState, MenuAction, MenuBarId, OpenMenuState, PendingMenuActions,
};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton, ThemedPanel};
use crate::ui::theme::{Palette, UiThemeAssets};

pub const MENU_BAR_HEIGHT: f32 = 26.0;

struct MenuDefinition {
    id: MenuBarId,
    label: &'static str,
    entries: &'static [(&'static str, MenuAction)],
}

const MENU_DEFINITIONS: &[MenuDefinition] = &[
    MenuDefinition {
        id: MenuBarId::File,
        label: "File",
        entries: &[("Logout", MenuAction::Logout), ("Quit", MenuAction::Quit)],
    },
    MenuDefinition {
        id: MenuBarId::View,
        label: "View",
        entries: &[
            ("Full Map  (M)", MenuAction::ToggleFullMap),
            ("Minimap", MenuAction::ToggleMinimap),
            ("Inventory", MenuAction::ToggleBackpack),
            ("Character", MenuAction::ToggleStatus),
            ("Equipment", MenuAction::ToggleEquipment),
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
                for (label, action) in definition.entries {
                    dropdown
                        .spawn((
                            Button,
                            ThemedButton::new(ButtonStyle::Ghost),
                            MenuDropdownEntryButton { action: *action },
                            Node {
                                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                                width: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(ghost_bg),
                        ))
                        .with_children(|entry| {
                            entry.spawn((
                                Text::new(*label),
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
            let center = transform.translation;
            let size = computed.size();
            let left = center.x - size.x * 0.5;
            node.left = Val::Px(left);
            node.top = Val::Px(MENU_BAR_HEIGHT);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn apply_menu_actions(
    mut commands: Commands,
    mut pending: ResMut<PendingMenuActions>,
    mut full_map_state: ResMut<FullMapWindowState>,
    mut panel_state: ResMut<DockedPanelState>,
    mut app_exit: MessageWriter<AppExit>,
    mut next_state: ResMut<NextState<ClientAppState>>,
    mut connection: Option<ResMut<TcpClientConnection>>,
    mut pending_saves: Option<ResMut<PendingPlayerSaves>>,
    local_players: Query<(Entity, &PlayerIdentity), With<Player>>,
) {
    for action in pending.actions.drain(..) {
        match action {
            MenuAction::ToggleFullMap => {
                full_map_state.open = !full_map_state.open;
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
        }
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
