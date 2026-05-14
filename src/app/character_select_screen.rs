//! Character Select screen. Shows the roster of characters owned by the
//! authenticated account and lets the user pick one to play, create a new
//! one, or delete an existing one.

use bevy::log::{info, warn};
use bevy::prelude::*;

use crate::app::plugin::AppRuntime;
use crate::app::state::ClientAppState;
use crate::network::protocol::{CharacterSummary, ClientMessage, ServerMessage};
use crate::network::resources::{TcpClientConfig, TcpClientConnection};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton, ThemedPanel};
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct CharacterSelectScreenPlugin {
    pub runtime: AppRuntime,
}

impl Plugin for CharacterSelectScreenPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CharacterSelectState::new(self.runtime))
            .add_systems(
                OnEnter(ClientAppState::CharacterSelect),
                (request_character_list, spawn_character_select_screen).chain(),
            )
            .add_systems(
                Update,
                (
                    poll_character_messages,
                    rebuild_roster_if_changed,
                    handle_character_select_buttons,
                )
                    .run_if(in_state(ClientAppState::CharacterSelect)),
            )
            .add_systems(
                OnExit(ClientAppState::CharacterSelect),
                cleanup_character_select_screen,
            );
    }
}

#[derive(Resource)]
pub struct CharacterSelectState {
    pub runtime: AppRuntime,
    pub characters: Vec<CharacterSummary>,
    pub list_dirty: bool,
    pub error_message: Option<String>,
    pub selected_character_id: Option<i64>,
}

impl CharacterSelectState {
    fn new(runtime: AppRuntime) -> Self {
        Self {
            runtime,
            characters: Vec::new(),
            list_dirty: true,
            error_message: None,
            selected_character_id: None,
        }
    }

    pub fn set_characters(&mut self, characters: Vec<CharacterSummary>) {
        self.characters = characters;
        self.list_dirty = true;
        if let Some(selected) = self.selected_character_id {
            if !self.characters.iter().any(|c| c.character_id == selected) {
                self.selected_character_id = None;
            }
        }
    }
}

#[derive(Component)]
struct CharacterSelectRoot;

#[derive(Component)]
struct CharacterListBody;

#[derive(Component, Clone, Copy)]
struct CharacterRowButton {
    character_id: i64,
}

#[derive(Component, Clone, Copy)]
struct CharacterDeleteButton {
    character_id: i64,
}

#[derive(Component, Clone, Copy, Eq, PartialEq)]
enum CharacterSelectAction {
    Play,
    CreateNew,
    Logout,
}

#[derive(Component)]
struct CharacterSelectActionButton {
    action: CharacterSelectAction,
}

#[derive(Component)]
struct CharacterSelectErrorText;

fn request_character_list(
    mut state: ResMut<CharacterSelectState>,
    config: Option<Res<TcpClientConfig>>,
    mut connection: Option<ResMut<TcpClientConnection>>,
    db: Option<Res<crate::accounts::AccountDbHandle>>,
) {
    match state.runtime {
        AppRuntime::TcpClient => {
            let (Some(config), Some(connection)) = (config, connection.as_deref_mut()) else {
                return;
            };
            crate::network::systems::ensure_tcp_client_connected(&config, connection);
            let Some(stream) = connection.stream.as_mut() else {
                return;
            };
            let mut disconnected = false;
            crate::network::systems::write_message(
                stream,
                &ClientMessage::ListCharacters,
                &mut disconnected,
            );
            if disconnected {
                connection.stream = None;
                connection.read_buffer.clear();
            }
        }
        AppRuntime::EmbeddedClient => {
            // Read directly from the in-process DB.
            let Some(db) = db.as_deref() else {
                return;
            };
            let list = {
                let guard = db.lock();
                guard
                    .list_characters(crate::accounts::LOCAL_ACCOUNT_ID)
                    .unwrap_or_default()
            };
            let summaries: Vec<CharacterSummary> = list
                .into_iter()
                .map(|s| CharacterSummary {
                    character_id: s.character_id,
                    name: s.name,
                    class: s.class,
                    level: s.level,
                })
                .collect();
            // If empty, jump to CharacterCreate immediately so the embedded
            // user isn't stuck on an empty roster.
            state.set_characters(summaries);
            state.error_message = None;
        }
        AppRuntime::HeadlessServer => {}
    }
}

fn spawn_character_select_screen(
    mut commands: Commands,
    state: Res<CharacterSelectState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
) {
    let theme = theme.clone();
    let palette = *palette;

    commands
        .spawn((
            CharacterSelectRoot,
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.03, 0.03, 0.96)),
        ))
        .with_children(|root| {
            root.spawn((
                ThemedPanel,
                Node {
                    width: px(560.0),
                    max_width: percent(96.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(16.0),
                    padding: UiRect::all(px(24.0)),
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
                    Text::new("Choose your character"),
                    TextFont {
                        font_size: 32.0,
                        ..default()
                    },
                    TextColor(palette.text_primary),
                ));

                panel.spawn((
                    Text::new("Select an existing character or create a new one."),
                    TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(palette.text_muted),
                ));

                // Roster container — rebuilt on list_dirty.
                panel
                    .spawn((
                        CharacterListBody,
                        Node {
                            width: percent(100.0),
                            min_height: px(120.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(8.0),
                            ..default()
                        },
                    ))
                    .with_children(|body| {
                        build_roster_rows(body, &theme, &palette, &state);
                    });

                panel.spawn((
                    CharacterSelectErrorText,
                    Text::new(state.error_message.clone().unwrap_or_default()),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.35, 0.35)),
                ));

                panel
                    .spawn((Node {
                        width: percent(100.0),
                        column_gap: px(12.0),
                        ..default()
                    },))
                    .with_children(|actions| {
                        spawn_action_button(
                            actions,
                            &theme,
                            &palette,
                            "Play",
                            CharacterSelectAction::Play,
                        );
                        spawn_action_button(
                            actions,
                            &theme,
                            &palette,
                            "Create new character",
                            CharacterSelectAction::CreateNew,
                        );
                        spawn_action_button(
                            actions,
                            &theme,
                            &palette,
                            "Log out",
                            CharacterSelectAction::Logout,
                        );
                    });
            });
        });
}

fn build_roster_rows(
    body: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    state: &CharacterSelectState,
) {
    if state.characters.is_empty() {
        body.spawn((
            Text::new("No characters yet. Click \"Create new character\" to make one."),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(palette.text_muted),
        ));
        return;
    }

    for summary in &state.characters {
        let selected = state.selected_character_id == Some(summary.character_id);
        let (bg, border, _text) = idle_colors(palette, ButtonStyle::Slot, selected);

        body.spawn((Node {
            width: percent(100.0),
            column_gap: px(8.0),
            align_items: AlignItems::Stretch,
            ..default()
        },))
            .with_children(|row| {
                row.spawn((
                    Button,
                    ThemedButton {
                        style: ButtonStyle::Slot,
                        selected,
                    },
                    CharacterRowButton {
                        character_id: summary.character_id,
                    },
                    Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(12.0)),
                        row_gap: px(4.0),
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
                        Text::new(summary.name.clone()),
                        TextFont {
                            font_size: 22.0,
                            ..default()
                        },
                        TextColor(palette.text_primary),
                    ));
                    button.spawn((
                        Text::new(format!(
                            "{} — Level {}",
                            summary.class.label(),
                            summary.level
                        )),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(palette.text_muted),
                    ));
                });

                let (del_bg, del_border, _) = idle_colors(palette, ButtonStyle::Danger, false);
                row.spawn((
                    Button,
                    ThemedButton::new(ButtonStyle::Danger),
                    CharacterDeleteButton {
                        character_id: summary.character_id,
                    },
                    Node {
                        width: px(36.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(px(6.0)),
                        border: UiRect::all(px(1.0)),
                        ..default()
                    },
                    ImageNode::new(theme.button_frame.clone())
                        .with_mode(theme.button_image_mode())
                        .with_color(del_bg),
                    BackgroundColor(Color::NONE),
                    BorderColor::all(del_border),
                ))
                .with_children(|button| {
                    button.spawn((
                        Text::new("X"),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(palette.text_primary),
                    ));
                });
            });
    }
}

fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    action: CharacterSelectAction,
) {
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Primary, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Primary),
            CharacterSelectActionButton { action },
            Node {
                flex_grow: 1.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(14.0), px(12.0)),
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
                    font_size: 18.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

fn poll_character_messages(
    config: Option<Res<TcpClientConfig>>,
    mut connection: Option<ResMut<TcpClientConnection>>,
    mut state: ResMut<CharacterSelectState>,
) {
    if !matches!(state.runtime, AppRuntime::TcpClient) {
        return;
    }
    let (Some(config), Some(connection)) = (config, connection.as_deref_mut()) else {
        return;
    };
    crate::network::systems::ensure_tcp_client_connected(&config, connection);
    let mut read_buffer = std::mem::take(&mut connection.read_buffer);
    let Some(stream) = connection.stream.as_mut() else {
        connection.read_buffer = read_buffer;
        return;
    };

    let mut disconnected = false;
    while let Some(line) =
        crate::network::systems::read_next_line(stream, &mut read_buffer, &mut disconnected)
    {
        match serde_json::from_str::<ServerMessage>(&line) {
            Ok(ServerMessage::CharacterList(list)) => {
                info!("character list: {} entries", list.len());
                state.set_characters(list);
                state.error_message = None;
            }
            Ok(ServerMessage::CharacterCreateResult {
                ok,
                character_id,
                reason,
            }) => {
                if ok {
                    info!("character created: {:?}", character_id);
                } else if let Some(reason) = reason {
                    warn!("character create rejected: {reason}");
                    state.error_message = Some(reason);
                }
            }
            Ok(_) => {
                // Other server messages aren't expected pre-select; ignore.
            }
            Err(err) => warn!("character select: failed to parse server message: {err}"),
        }
    }

    if disconnected {
        warn!("character select: lost TCP connection");
        connection.stream = None;
        connection.read_buffer.clear();
    } else {
        connection.read_buffer = read_buffer;
    }
}

fn rebuild_roster_if_changed(
    mut state: ResMut<CharacterSelectState>,
    mut commands: Commands,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    body_query: Query<Entity, With<CharacterListBody>>,
    children_query: Query<&Children>,
    mut error_text: Query<&mut Text, With<CharacterSelectErrorText>>,
) {
    if state.list_dirty {
        for body in body_query.iter() {
            // Despawn existing children.
            if let Ok(children) = children_query.get(body) {
                for child in children.iter() {
                    commands.entity(child).despawn();
                }
            }
            commands.entity(body).with_children(|body| {
                build_roster_rows(body, &theme, &palette, &state);
            });
        }
        state.list_dirty = false;
    }
    // Always sync the error text in case it changed.
    let desired = state.error_message.clone().unwrap_or_default();
    for mut text in &mut error_text {
        if text.0 != desired {
            text.0 = desired.clone();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_character_select_buttons(
    mut state: ResMut<CharacterSelectState>,
    mut next_state: ResMut<NextState<ClientAppState>>,
    config: Option<Res<TcpClientConfig>>,
    mut connection: Option<ResMut<TcpClientConnection>>,
    db: Option<Res<crate::accounts::AccountDbHandle>>,
    mut local_selected: Option<ResMut<crate::app::state::LocalSelectedCharacter>>,
    row_buttons: Query<(&Interaction, &CharacterRowButton), (Changed<Interaction>, With<Button>)>,
    delete_buttons: Query<
        (&Interaction, &CharacterDeleteButton),
        (Changed<Interaction>, With<Button>),
    >,
    action_buttons: Query<
        (&Interaction, &CharacterSelectActionButton),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, button) in &row_buttons {
        if *interaction == Interaction::Pressed {
            state.selected_character_id = Some(button.character_id);
            state.list_dirty = true;
        }
    }

    let mut delete_target: Option<i64> = None;
    for (interaction, button) in &delete_buttons {
        if *interaction == Interaction::Pressed {
            delete_target = Some(button.character_id);
        }
    }
    if let Some(character_id) = delete_target {
        match state.runtime {
            AppRuntime::TcpClient => {
                send_message(
                    config.as_deref(),
                    connection.as_deref_mut(),
                    ClientMessage::DeleteCharacter { character_id },
                );
            }
            AppRuntime::EmbeddedClient => {
                if let Some(db) = db.as_deref() {
                    let _ = db
                        .lock()
                        .delete_character(crate::accounts::LOCAL_ACCOUNT_ID, character_id);
                    let list = db
                        .lock()
                        .list_characters(crate::accounts::LOCAL_ACCOUNT_ID)
                        .unwrap_or_default();
                    let summaries: Vec<CharacterSummary> = list
                        .into_iter()
                        .map(|s| CharacterSummary {
                            character_id: s.character_id,
                            name: s.name,
                            class: s.class,
                            level: s.level,
                        })
                        .collect();
                    state.set_characters(summaries);
                }
            }
            AppRuntime::HeadlessServer => {}
        }
        if state.selected_character_id == Some(character_id) {
            state.selected_character_id = None;
        }
    }

    for (interaction, button) in &action_buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button.action {
            CharacterSelectAction::Play => {
                let Some(character_id) = state.selected_character_id else {
                    state.error_message = Some("select a character first".to_owned());
                    continue;
                };
                match state.runtime {
                    AppRuntime::TcpClient => {
                        send_message(
                            config.as_deref(),
                            connection.as_deref_mut(),
                            ClientMessage::SelectCharacter { character_id },
                        );
                        info!("requested SelectCharacter {character_id}");
                        next_state.set(ClientAppState::AssetSync);
                    }
                    AppRuntime::EmbeddedClient => {
                        if let Some(local) = local_selected.as_deref_mut() {
                            local.character_id = Some(character_id);
                        }
                        info!("embedded selected character {character_id}");
                        next_state.set(ClientAppState::InGame);
                    }
                    AppRuntime::HeadlessServer => {}
                }
            }
            CharacterSelectAction::CreateNew => {
                state.error_message = None;
                next_state.set(ClientAppState::CharacterCreate);
            }
            CharacterSelectAction::Logout => {
                if let Some(connection) = connection.as_deref_mut() {
                    connection.stream = None;
                    connection.read_buffer.clear();
                }
                state.set_characters(Vec::new());
                state.error_message = None;
                state.selected_character_id = None;
                next_state.set(ClientAppState::TitleScreen);
            }
        }
    }
}

fn send_message(
    config: Option<&TcpClientConfig>,
    mut connection: Option<&mut TcpClientConnection>,
    msg: ClientMessage,
) {
    let (Some(config), Some(connection)) = (config, connection.as_deref_mut()) else {
        return;
    };
    crate::network::systems::ensure_tcp_client_connected(config, connection);
    let Some(stream) = connection.stream.as_mut() else {
        return;
    };
    let mut disconnected = false;
    crate::network::systems::write_message(stream, &msg, &mut disconnected);
    if disconnected {
        connection.stream = None;
        connection.read_buffer.clear();
    }
}

fn cleanup_character_select_screen(
    mut commands: Commands,
    root_query: Query<Entity, With<CharacterSelectRoot>>,
) {
    for entity in &root_query {
        commands.entity(entity).despawn();
    }
}
