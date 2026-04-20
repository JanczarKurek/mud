use bevy::app::AppExit;
use bevy::prelude::*;

use crate::app::plugin::AppRuntime;
use crate::app::state::ClientAppState;
use crate::network::resources::TcpClientConfig;
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton, ThemedPanel};
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct TitleScreenPlugin {
    pub runtime: AppRuntime,
    pub server_addr: Option<String>,
}

impl Plugin for TitleScreenPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TitleScreenState::new(
            self.runtime,
            self.server_addr.clone(),
        ))
        .add_systems(OnEnter(ClientAppState::TitleScreen), spawn_title_screen)
        .add_systems(
            Update,
            (sync_server_selection_buttons, handle_title_screen_buttons)
                .run_if(in_state(ClientAppState::TitleScreen)),
        )
        .add_systems(OnExit(ClientAppState::TitleScreen), cleanup_title_screen);
    }
}

#[derive(Clone, Debug)]
struct TitleServerEntry {
    label: String,
    description: String,
    server_addr: Option<String>,
}

#[derive(Resource)]
struct TitleScreenState {
    runtime: AppRuntime,
    entries: Vec<TitleServerEntry>,
    selected_index: usize,
}

impl TitleScreenState {
    fn new(runtime: AppRuntime, server_addr: Option<String>) -> Self {
        let mut entries = Vec::new();

        match runtime {
            AppRuntime::EmbeddedClient => {
                entries.push(TitleServerEntry {
                    label: "Embedded Realm".to_owned(),
                    description: "Run the local in-process world.".to_owned(),
                    server_addr: None,
                });
            }
            AppRuntime::TcpClient => {
                entries.push(TitleServerEntry {
                    label: "Localhost".to_owned(),
                    description: "Connect to 127.0.0.1:7000.".to_owned(),
                    server_addr: Some("127.0.0.1:7000".to_owned()),
                });

                if let Some(server_addr) = server_addr.filter(|addr| addr != "127.0.0.1:7000") {
                    entries.push(TitleServerEntry {
                        label: "CLI Server".to_owned(),
                        description: format!("Connect to {server_addr}."),
                        server_addr: Some(server_addr),
                    });
                }
            }
            AppRuntime::HeadlessServer => {}
        }

        Self {
            runtime,
            entries,
            selected_index: 0,
        }
    }
}

#[derive(Component)]
struct TitleScreenRoot;

#[derive(Component)]
struct TitleServerButton {
    index: usize,
}

#[derive(Component, Clone, Copy, Eq, PartialEq)]
enum TitleAction {
    Connect,
    OpenMapEditor,
    Exit,
}

#[derive(Component)]
struct TitleActionButton {
    action: TitleAction,
}

fn spawn_title_screen(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    title_state: Res<TitleScreenState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
) {
    let theme = theme.clone();
    let palette = *palette;

    commands
        .spawn((
            TitleScreenRoot,
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: percent(100.0),
                    height: percent(100.0),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                ImageNode::new(asset_server.load("ui/title_screen/splash.png")),
            ));

            root.spawn((
                Node {
                    width: percent(100.0),
                    height: percent(100.0),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.02, 0.02, 0.45)),
            ));

            root.spawn((Node {
                width: percent(100.0),
                height: percent(100.0),
                padding: UiRect::axes(px(42.0), px(28.0)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::End,
                ..default()
            },))
                .with_children(|layout| {
                    layout
                        .spawn((
                            ThemedPanel,
                            Node {
                                width: px(520.0),
                                max_width: percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(18.0),
                                padding: UiRect::all(px(24.0)),
                                border: UiRect::all(px(1.0)),
                                ..default()
                            },
                            ImageNode::new(theme.panel_frame.clone())
                                .with_mode(theme.panel_image_mode())
                                .with_color(palette.surface_panel),
                            BackgroundColor(Color::NONE),
                            BorderColor::all(palette.border_accent),
                        ))
                        .with_children(|panel| {
                            panel.spawn((
                                Text::new("Mud 2.0"),
                                TextFont {
                                    font_size: 46.0,
                                    ..default()
                                },
                                TextColor(palette.text_primary),
                            ));

                            panel.spawn((
                                Text::new("Choose a realm, then enter the client."),
                                TextFont {
                                    font_size: 18.0,
                                    ..default()
                                },
                                TextColor(palette.text_muted),
                            ));

                            panel
                                .spawn((Node {
                                    width: percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(10.0),
                                    ..default()
                                },))
                                .with_children(|server_list| {
                                    server_list.spawn((
                                        Text::new("Servers"),
                                        TextFont {
                                            font_size: 20.0,
                                            ..default()
                                        },
                                        TextColor(palette.text_accent),
                                    ));

                                    for (index, entry) in title_state.entries.iter().enumerate() {
                                        let selected = index == title_state.selected_index;
                                        let (bg, border, _text) =
                                            idle_colors(&palette, ButtonStyle::Slot, selected);
                                        server_list
                                            .spawn((
                                                Button,
                                                ThemedButton {
                                                    style: ButtonStyle::Slot,
                                                    selected,
                                                },
                                                TitleServerButton { index },
                                                Node {
                                                    width: percent(100.0),
                                                    flex_direction: FlexDirection::Column,
                                                    align_items: AlignItems::Start,
                                                    padding: UiRect::all(px(14.0)),
                                                    border: UiRect::all(px(1.0)),
                                                    row_gap: px(4.0),
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
                                                    Text::new(entry.label.clone()),
                                                    TextFont {
                                                        font_size: 22.0,
                                                        ..default()
                                                    },
                                                    TextColor(palette.text_primary),
                                                ));
                                                button.spawn((
                                                    Text::new(entry.description.clone()),
                                                    TextFont {
                                                        font_size: 16.0,
                                                        ..default()
                                                    },
                                                    TextColor(palette.text_muted),
                                                ));
                                            });
                                    }
                                });

                            panel
                                .spawn((Node {
                                    width: percent(100.0),
                                    justify_content: JustifyContent::SpaceBetween,
                                    column_gap: px(14.0),
                                    ..default()
                                },))
                                .with_children(|footer| {
                                    footer
                                        .spawn((
                                            ThemedPanel,
                                            Node {
                                                width: percent(58.0),
                                                flex_direction: FlexDirection::Column,
                                                row_gap: px(8.0),
                                                padding: UiRect::all(px(14.0)),
                                                border: UiRect::all(px(1.0)),
                                                ..default()
                                            },
                                            ImageNode::new(theme.panel_frame.clone())
                                                .with_mode(theme.panel_image_mode())
                                                .with_color(palette.surface_panel),
                                            BackgroundColor(Color::NONE),
                                            BorderColor::all(palette.border_idle),
                                        ))
                                        .with_children(|authors| {
                                            authors.spawn((
                                                Text::new("Authors"),
                                                TextFont {
                                                    font_size: 20.0,
                                                    ..default()
                                                },
                                                TextColor(palette.text_accent),
                                            ));
                                            authors.spawn((
                                                Text::new("1. Codex\n2. Janczar Knurek"),
                                                TextFont {
                                                    font_size: 18.0,
                                                    ..default()
                                                },
                                                TextColor(palette.text_value),
                                            ));
                                        });

                                    footer
                                        .spawn((Node {
                                            width: percent(42.0),
                                            flex_direction: FlexDirection::Column,
                                            justify_content: JustifyContent::End,
                                            row_gap: px(10.0),
                                            ..default()
                                        },))
                                        .with_children(|actions| {
                                            spawn_action_button(
                                                actions,
                                                &theme,
                                                &palette,
                                                "Connect",
                                                TitleAction::Connect,
                                            );
                                            if title_state.runtime == AppRuntime::EmbeddedClient {
                                                spawn_action_button(
                                                    actions,
                                                    &theme,
                                                    &palette,
                                                    "Map Editor",
                                                    TitleAction::OpenMapEditor,
                                                );
                                            }
                                            spawn_action_button(
                                                actions,
                                                &theme,
                                                &palette,
                                                "Exit",
                                                TitleAction::Exit,
                                            );
                                        });
                                });
                        });

                    layout.spawn((
                        Text::new("Splash art: assets/ui/title_screen/splash.png"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgba(0.95, 0.94, 0.88, 0.90)),
                    ));
                });
        });
}

fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    action: TitleAction,
) {
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Primary, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Primary),
            TitleActionButton { action },
            Node {
                width: percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(18.0), px(14.0)),
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
                    font_size: 22.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

/// Keeps `ThemedButton.selected` in sync with `TitleScreenState.selected_index`
/// so the shared recolor system shows the active server with its selected tint.
fn sync_server_selection_buttons(
    title_state: Res<TitleScreenState>,
    mut button_query: Query<(&TitleServerButton, &mut ThemedButton), With<Button>>,
) {
    for (button, mut themed) in &mut button_query {
        themed.selected = button.index == title_state.selected_index;
    }
}

fn handle_title_screen_buttons(
    mut title_state: ResMut<TitleScreenState>,
    mut next_state: ResMut<NextState<ClientAppState>>,
    mut tcp_config: Option<ResMut<TcpClientConfig>>,
    mut exit_messages: MessageWriter<AppExit>,
    server_buttons: Query<(&Interaction, &TitleServerButton), (Changed<Interaction>, With<Button>)>,
    action_buttons: Query<(&Interaction, &TitleActionButton), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, button) in &server_buttons {
        if *interaction == Interaction::Pressed {
            title_state.selected_index = button.index;
        }
    }

    for (interaction, button) in &action_buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }

        match button.action {
            TitleAction::Connect => {
                if let Some(tcp_config) = tcp_config.as_mut() {
                    if let Some(server_addr) = title_state
                        .entries
                        .get(title_state.selected_index)
                        .and_then(|entry| entry.server_addr.clone())
                    {
                        tcp_config.server_addr = server_addr;
                    }
                    tcp_config.active = true;
                    next_state.set(ClientAppState::AssetSync);
                } else {
                    next_state.set(ClientAppState::InGame);
                }
            }
            TitleAction::OpenMapEditor => {
                next_state.set(ClientAppState::MapEditor);
            }
            TitleAction::Exit => {
                exit_messages.write(AppExit::Success);
            }
        }
    }
}

fn cleanup_title_screen(mut commands: Commands, root_query: Query<Entity, With<TitleScreenRoot>>) {
    for entity in &root_query {
        commands.entity(entity).despawn();
    }
}
