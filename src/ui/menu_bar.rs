use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};

use crate::ui::components::{
    MenuBarItemButton, MenuBarRoot, MenuDropdownEntryButton, MenuDropdownRoot,
};
use crate::ui::resources::{
    DockedPanelKind, DockedPanelState, FullMapWindowState, MenuAction, MenuBarId, OpenMenuState,
    PendingMenuActions,
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
        entries: &[("Quit", MenuAction::Quit)],
    },
    MenuDefinition {
        id: MenuBarId::View,
        label: "View",
        entries: &[
            ("Full Map  (M)", MenuAction::ToggleFullMap),
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
                ImageNode::new(theme.panel_frame.clone())
                    .with_mode(theme.panel_image_mode())
                    .with_color(palette.surface_panel),
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

pub fn apply_menu_actions(
    mut pending: ResMut<PendingMenuActions>,
    mut full_map_state: ResMut<FullMapWindowState>,
    mut panel_state: ResMut<DockedPanelState>,
    mut app_exit: MessageWriter<AppExit>,
) {
    for action in pending.actions.drain(..) {
        match action {
            MenuAction::ToggleFullMap => {
                full_map_state.open = !full_map_state.open;
            }
            MenuAction::ToggleStatus => {
                toggle_panel(&mut panel_state, DockedPanelState::STATUS_PANEL_ID);
            }
            MenuAction::ToggleBackpack => {
                toggle_panel(&mut panel_state, DockedPanelState::BACKPACK_PANEL_ID);
            }
            MenuAction::ToggleEquipment => {
                toggle_panel(&mut panel_state, DockedPanelState::EQUIPMENT_PANEL_ID);
            }
            MenuAction::Quit => {
                app_exit.write(AppExit::Success);
            }
        }
    }
}

fn toggle_panel(panel_state: &mut DockedPanelState, panel_id: usize) {
    if panel_state.is_open(panel_id) {
        panel_state.close_panel(panel_id);
        return;
    }

    let (kind, title, height) = match panel_id {
        DockedPanelState::STATUS_PANEL_ID => (
            DockedPanelKind::Status,
            "Status",
            DockedPanelState::DEFAULT_STATUS_PANEL_HEIGHT,
        ),
        DockedPanelState::EQUIPMENT_PANEL_ID => (
            DockedPanelKind::Equipment,
            "Equipment",
            DockedPanelState::DEFAULT_EQUIPMENT_PANEL_HEIGHT,
        ),
        DockedPanelState::BACKPACK_PANEL_ID => (
            DockedPanelKind::Backpack,
            "Backpack",
            DockedPanelState::DEFAULT_BACKPACK_PANEL_HEIGHT,
        ),
        _ => return,
    };

    panel_state.panels.push(crate::ui::resources::DockedPanel {
        id: panel_id,
        kind,
        title: title.to_owned(),
        height,
        closable: true,
        resizable: true,
        movable: true,
    });
}
