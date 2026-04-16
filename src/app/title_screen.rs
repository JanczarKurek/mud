use bevy::app::AppExit;
use bevy::prelude::*;

use crate::app::plugin::AppRuntime;
use crate::app::state::ClientAppState;
use crate::network::resources::TcpClientConfig;

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
            (
                sync_server_selection_buttons,
                sync_title_action_buttons,
                handle_title_screen_buttons,
            )
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
) {
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
                            Node {
                                width: px(520.0),
                                max_width: percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(18.0),
                                padding: UiRect::all(px(24.0)),
                                border: UiRect::all(px(1.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.08, 0.05, 0.05, 0.84)),
                            BorderColor::all(Color::srgb(0.72, 0.59, 0.41)),
                        ))
                        .with_children(|panel| {
                            panel.spawn((
                                Text::new("Mud 2.0"),
                                TextFont {
                                    font_size: 46.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.95, 0.90, 0.78)),
                            ));

                            panel.spawn((
                                Text::new("Choose a realm, then enter the client."),
                                TextFont {
                                    font_size: 18.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.84, 0.80, 0.72)),
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
                                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                                    ));

                                    for (index, entry) in title_state.entries.iter().enumerate() {
                                        server_list
                                            .spawn((
                                                Button,
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
                                                BorderColor::all(Color::srgb(0.40, 0.31, 0.22)),
                                                BackgroundColor(Color::srgba(
                                                    0.14, 0.10, 0.10, 0.94,
                                                )),
                                            ))
                                            .with_children(|button| {
                                                button.spawn((
                                                    Text::new(entry.label.clone()),
                                                    TextFont {
                                                        font_size: 22.0,
                                                        ..default()
                                                    },
                                                    TextColor(Color::srgb(0.95, 0.91, 0.83)),
                                                ));
                                                button.spawn((
                                                    Text::new(entry.description.clone()),
                                                    TextFont {
                                                        font_size: 16.0,
                                                        ..default()
                                                    },
                                                    TextColor(Color::srgb(0.80, 0.76, 0.70)),
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
                                            Node {
                                                width: percent(58.0),
                                                flex_direction: FlexDirection::Column,
                                                row_gap: px(8.0),
                                                padding: UiRect::all(px(14.0)),
                                                border: UiRect::all(px(1.0)),
                                                ..default()
                                            },
                                            BackgroundColor(Color::srgba(0.10, 0.08, 0.08, 0.88)),
                                            BorderColor::all(Color::srgb(0.36, 0.29, 0.20)),
                                        ))
                                        .with_children(|authors| {
                                            authors.spawn((
                                                Text::new("Authors"),
                                                TextFont {
                                                    font_size: 20.0,
                                                    ..default()
                                                },
                                                TextColor(Color::srgb(0.96, 0.84, 0.62)),
                                            ));
                                            authors.spawn((
                                                Text::new("1. Codex\n2. Janczar Knurek"),
                                                TextFont {
                                                    font_size: 18.0,
                                                    ..default()
                                                },
                                                TextColor(Color::srgb(0.89, 0.86, 0.80)),
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
                                                "Connect",
                                                TitleAction::Connect,
                                            );
                                            if title_state.runtime == AppRuntime::EmbeddedClient {
                                                spawn_action_button(
                                                    actions,
                                                    "Map Editor",
                                                    TitleAction::OpenMapEditor,
                                                );
                                            }
                                            spawn_action_button(actions, "Exit", TitleAction::Exit);
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

fn spawn_action_button(parent: &mut ChildSpawnerCommands, label: &str, action: TitleAction) {
    parent
        .spawn((
            Button,
            TitleActionButton { action },
            Node {
                width: percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(18.0), px(14.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.48, 0.36, 0.24)),
            BackgroundColor(Color::srgba(0.18, 0.12, 0.10, 0.96)),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
                TextColor(Color::srgb(0.96, 0.92, 0.82)),
            ));
        });
}

fn sync_server_selection_buttons(
    title_state: Res<TitleScreenState>,
    mut button_query: Query<
        (
            &TitleServerButton,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Button>,
    >,
) {
    for (button, interaction, mut background, mut border) in &mut button_query {
        let is_selected = button.index == title_state.selected_index;
        let (background_color, border_color) = match (*interaction, is_selected) {
            (Interaction::Pressed, _) => {
                (Color::srgb(0.54, 0.31, 0.17), Color::srgb(0.98, 0.85, 0.60))
            }
            (Interaction::Hovered, true) => {
                (Color::srgb(0.40, 0.22, 0.12), Color::srgb(0.98, 0.85, 0.60))
            }
            (Interaction::Hovered, false) => {
                (Color::srgb(0.25, 0.16, 0.12), Color::srgb(0.84, 0.68, 0.45))
            }
            (Interaction::None, true) => {
                (Color::srgb(0.30, 0.17, 0.10), Color::srgb(0.92, 0.78, 0.55))
            }
            (Interaction::None, false) => {
                (Color::srgb(0.14, 0.10, 0.10), Color::srgb(0.40, 0.31, 0.22))
            }
        };

        background.0 = background_color;
        *border = BorderColor::all(border_color);
    }
}

fn sync_title_action_buttons(
    mut button_query: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (With<Button>, With<TitleActionButton>),
    >,
) {
    for (interaction, mut background, mut border) in &mut button_query {
        let (background_color, border_color) = match *interaction {
            Interaction::Pressed => (Color::srgb(0.62, 0.32, 0.14), Color::srgb(1.0, 0.88, 0.64)),
            Interaction::Hovered => (Color::srgb(0.34, 0.18, 0.10), Color::srgb(0.92, 0.78, 0.55)),
            Interaction::None => (Color::srgb(0.18, 0.12, 0.10), Color::srgb(0.48, 0.36, 0.24)),
        };

        background.0 = background_color;
        *border = BorderColor::all(border_color);
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
                }

                next_state.set(ClientAppState::InGame);
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
