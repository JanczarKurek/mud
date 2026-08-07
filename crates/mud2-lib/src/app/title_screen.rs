use bevy::app::AppExit;
use bevy::ecs::message::MessageReader;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

use crate::app::auth_screen::PendingAuthRequest;
use crate::app::plugin::AppRuntime;
use crate::app::state::{ClientAppState, DebugMode};
use crate::network::resources::{TcpClientConfig, TcpClientConnection};
use crate::ui::settings::{SavedServerList, SelectedStartingMap};
use crate::ui::theme::text_field::{
    apply_text_edit, spawn_text_field, CharPolicy, TextEditOutcome, TextFieldStyle, TextFieldVisual,
};
use crate::ui::theme::widgets::{
    idle_colors, spawn_themed_button, ButtonStyle, ThemedButton, ThemedPanel,
};
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::world::map_layout::SpaceDefinitions;

pub struct TitleScreenPlugin {
    pub runtime: AppRuntime,
}

impl Plugin for TitleScreenPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TitleScreenState::new(self.runtime))
            .add_systems(
                OnEnter(ClientAppState::TitleScreen),
                (refresh_current_server, spawn_title_screen).chain(),
            )
            .add_systems(
                Update,
                (
                    handle_title_field_clicks,
                    handle_title_field_keyboard,
                    sync_title_field_text,
                    sync_title_field_focus_style,
                    sync_auth_error_text,
                    sync_clean_state_label,
                    handle_title_screen_buttons,
                    sync_current_server_card,
                    sync_server_picker_modal,
                    handle_server_picker_buttons,
                    sync_map_picker_modal,
                    handle_map_picker_buttons,
                )
                    .run_if(in_state(ClientAppState::TitleScreen)),
            )
            .add_systems(OnExit(ClientAppState::TitleScreen), cleanup_title_screen);
    }
}

/// One row the user can pick in the server-picker modal — also used as the
/// "current selection" indicator shown on the title screen. Origin is intentionally
/// flat (no enum); the picker only cares about label/description/addr.
#[derive(Clone, Debug)]
struct TitleServerEntry {
    label: String,
    description: String,
    server_addr: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoginField {
    Username,
    Password,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectField {
    Host,
    Port,
}

#[derive(Resource)]
struct TitleScreenState {
    runtime: AppRuntime,
    /// Currently chosen server — `None` only briefly in `HeadlessServer` mode
    /// (which never reaches this screen) or before settings finish loading.
    current: Option<TitleServerEntry>,
    /// Whether the server-picker modal is on screen.
    modal_open: bool,
    /// Whether the starting-map picker modal is on screen (embedded only).
    map_picker_open: bool,
    /// Direct-connect inputs (transient — never written to disk).
    direct_host: String,
    direct_port: String,
    direct_focused: Option<DirectField>,
    username: String,
    password: String,
    register: bool,
    focused: LoginField,
    /// Debug-only "Clean game state" two-click guard: first click arms it,
    /// second click within the same session performs the wipe + exit.
    confirm_clean: bool,
}

impl TitleScreenState {
    fn new(runtime: AppRuntime) -> Self {
        // Embedded has a single fixed "server"; populate it immediately so the
        // card has something to show before settings load runs. TCP modes get
        // their current selection set by `refresh_current_server` on
        // `OnEnter(TitleScreen)` after `SavedServerList` is loaded.
        let current = match runtime {
            AppRuntime::EmbeddedClient => Some(embedded_entry()),
            _ => None,
        };

        Self {
            runtime,
            current,
            modal_open: false,
            map_picker_open: false,
            direct_host: String::new(),
            direct_port: String::new(),
            direct_focused: None,
            username: String::new(),
            password: String::new(),
            register: false,
            focused: LoginField::Username,
            confirm_clean: false,
        }
    }

    fn is_tcp(&self) -> bool {
        matches!(self.runtime, AppRuntime::TcpClient)
    }
}

fn embedded_entry() -> TitleServerEntry {
    TitleServerEntry {
        label: "Embedded Realm".to_owned(),
        description: "Run the local in-process world.".to_owned(),
        server_addr: None,
    }
}

fn saved_entry_to_title(name: &str, addr: &str) -> TitleServerEntry {
    TitleServerEntry {
        label: name.to_owned(),
        description: format!("Connect to {addr}."),
        server_addr: Some(addr.to_owned()),
    }
}

/// Build the list of pickable entries for TCP mode from the saved-server list.
fn build_picker_entries(saved: &SavedServerList) -> Vec<TitleServerEntry> {
    saved
        .saved
        .iter()
        .map(|e| saved_entry_to_title(&e.name, &e.addr))
        .collect()
}

/// `OnEnter(TitleScreen)`: ensure `current` is populated for TCP mode using
/// the loaded `SavedServerList`. Honours `selected_addr` (the last picked
/// saved entry, persisted across launches); falls back to the first entry.
fn refresh_current_server(mut title_state: ResMut<TitleScreenState>, saved: Res<SavedServerList>) {
    if !matches!(title_state.runtime, AppRuntime::TcpClient) {
        return;
    }

    let entries = build_picker_entries(&saved);

    let still_valid = title_state
        .current
        .as_ref()
        .and_then(|cur| cur.server_addr.as_deref())
        .map(|addr| {
            entries
                .iter()
                .any(|e| e.server_addr.as_deref() == Some(addr))
        })
        .unwrap_or(false);

    if !still_valid {
        // First try the persisted last-picked addr; fall back to the first
        // saved entry (usually "Local").
        let remembered = saved.selected_addr.as_deref().and_then(|addr| {
            entries
                .iter()
                .find(|e| e.server_addr.as_deref() == Some(addr))
                .cloned()
        });
        title_state.current = remembered.or_else(|| entries.into_iter().next());
    }
}

#[derive(Component)]
struct TitleScreenRoot;

/// Root of the server-picker modal overlay; despawned when the modal closes.
#[derive(Component)]
struct ServerPickerModalRoot;

/// Root of the starting-map picker modal overlay; despawned when it closes.
#[derive(Component)]
struct MapPickerModalRoot;

/// One starting-map picker row; clicking it sets the selected starting map to
/// the carried authored id and closes the modal.
#[derive(Component, Clone)]
struct MapPickerEntryButton {
    map_id: String,
}

/// The map-picker "Cancel" button — closes the modal without changing the pick.
#[derive(Component)]
struct MapPickerCancel;

/// The single "current server" card on the title panel — clicking it opens
/// the picker modal (TCP mode only; no-op in Embedded mode).
#[derive(Component)]
struct CurrentServerCard;

/// Text labels inside the current-server card, kept in sync with the chosen
/// entry by `sync_current_server_card`.
#[derive(Component, Clone, Copy)]
enum CurrentServerCardText {
    Label,
    Description,
}

/// One picker-modal row; clicking it sets the current server to the carried
/// entry data and closes the modal.
#[derive(Component, Clone)]
struct PickerEntryButton {
    entry: TitleServerEntry,
}

/// Modal actions that aren't entry-selection clicks.
#[derive(Component, Clone, Copy, Eq, PartialEq)]
enum PickerAction {
    UseDirect,
    Cancel,
}

/// One title-screen text field — the login form or the direct-connect modal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TitleField {
    Login(LoginField),
    Direct(DirectField),
}

/// Click-to-focus box of a title-screen field; also the recolored border.
#[derive(Component, Clone, Copy)]
struct TitleFieldBox(TitleField);

/// Text node mirroring a title-screen field's value.
#[derive(Component, Clone, Copy)]
struct TitleFieldText(TitleField);

#[derive(Component, Clone, Copy, Eq, PartialEq)]
enum TitleAction {
    Connect,
    OpenServerPicker,
    ToggleRegister,
    OpenMapEditor,
    /// Embedded-only: open the starting-map picker modal.
    OpenMapPicker,
    OpenSettings,
    OpenAbout,
    /// Debug-only: wipe the embedded world snapshot + accounts DB on next boot,
    /// then exit. Two-click confirm via `TitleScreenState::confirm_clean`.
    CleanGameState,
    Exit,
}

#[derive(Component)]
struct TitleActionButton {
    action: TitleAction,
}

/// Label text of the "Clean game state" button — retargeted by
/// `sync_clean_state_label` to reflect the two-click confirm state.
#[derive(Component)]
struct CleanStateLabel;

/// Text node that displays the last auth error, if any.
#[derive(Component)]
struct AuthErrorText;

/// Text inside the Register toggle button (to flip the checkbox glyph).
#[derive(Component)]
struct RegisterToggleText;

fn spawn_title_screen(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    title_state: Res<TitleScreenState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    debug: Option<Res<DebugMode>>,
) {
    let theme = theme.clone();
    let palette = *palette;
    let debug_enabled = debug.is_some_and(|m| m.0);

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
                                .with_color(Color::WHITE),
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
                                    row_gap: px(8.0),
                                    ..default()
                                },))
                                .with_children(|server_section| {
                                    server_section.spawn((
                                        Text::new("Server"),
                                        TextFont {
                                            font_size: 20.0,
                                            ..default()
                                        },
                                        TextColor(palette.text_accent),
                                    ));

                                    let (label, description) = title_state
                                        .current
                                        .as_ref()
                                        .map(|e| (e.label.clone(), e.description.clone()))
                                        .unwrap_or_else(|| {
                                            ("No server selected".to_owned(), String::new())
                                        });
                                    let (bg, border, _text) =
                                        idle_colors(&palette, ButtonStyle::Slot, true);
                                    server_section
                                        .spawn((
                                            Button,
                                            ThemedButton {
                                                style: ButtonStyle::Slot,
                                                selected: true,
                                            },
                                            CurrentServerCard,
                                            TitleActionButton {
                                                action: TitleAction::OpenServerPicker,
                                            },
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
                                        .with_children(|card| {
                                            card.spawn((
                                                CurrentServerCardText::Label,
                                                Text::new(label),
                                                TextFont {
                                                    font_size: 22.0,
                                                    ..default()
                                                },
                                                TextColor(palette.text_primary),
                                            ));
                                            card.spawn((
                                                CurrentServerCardText::Description,
                                                Text::new(description),
                                                TextFont {
                                                    font_size: 16.0,
                                                    ..default()
                                                },
                                                TextColor(palette.text_muted),
                                            ));
                                            if title_state.is_tcp() {
                                                card.spawn((
                                                    Text::new("Click to change…"),
                                                    TextFont {
                                                        font_size: 13.0,
                                                        ..default()
                                                    },
                                                    TextColor(palette.text_muted),
                                                ));
                                            }
                                        });
                                });

                            if title_state.is_tcp() {
                                panel
                                    .spawn((Node {
                                        width: percent(100.0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(8.0),
                                        ..default()
                                    },))
                                    .with_children(|login| {
                                        login.spawn((
                                            Text::new("Account"),
                                            TextFont {
                                                font_size: 20.0,
                                                ..default()
                                            },
                                            TextColor(palette.text_accent),
                                        ));

                                        spawn_title_field(
                                            login,
                                            &palette,
                                            TitleField::Login(LoginField::Username),
                                            "Username",
                                            &title_state.username,
                                            title_state.focused == LoginField::Username,
                                            false,
                                        );
                                        spawn_title_field(
                                            login,
                                            &palette,
                                            TitleField::Login(LoginField::Password),
                                            "Password",
                                            &title_state.password,
                                            title_state.focused == LoginField::Password,
                                            true,
                                        );

                                        login
                                            .spawn((
                                                Button,
                                                TitleActionButton {
                                                    action: TitleAction::ToggleRegister,
                                                },
                                                Node {
                                                    padding: UiRect::axes(px(10.0), px(6.0)),
                                                    border: UiRect::all(px(1.0)),
                                                    ..default()
                                                },
                                                BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.5)),
                                                BorderColor::all(palette.border_idle),
                                            ))
                                            .with_children(|button| {
                                                button.spawn((
                                                    RegisterToggleText,
                                                    Text::new(register_label(title_state.register)),
                                                    TextFont {
                                                        font_size: 16.0,
                                                        ..default()
                                                    },
                                                    TextColor(palette.text_primary),
                                                ));
                                            });

                                        login.spawn((
                                            AuthErrorText,
                                            Text::new(""),
                                            TextFont {
                                                font_size: 14.0,
                                                ..default()
                                            },
                                            TextColor(Color::srgb(0.95, 0.35, 0.35)),
                                        ));
                                    });
                            }

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
                                                .with_color(Color::WHITE),
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
                                                Text::new("1. Claude\n2. Codex\n3. Janczar Knurek"),
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
                                                    "Select Map",
                                                    TitleAction::OpenMapPicker,
                                                );
                                                spawn_action_button(
                                                    actions,
                                                    &theme,
                                                    &palette,
                                                    "Map Editor",
                                                    TitleAction::OpenMapEditor,
                                                );
                                                if debug_enabled {
                                                    spawn_clean_state_button(
                                                        actions, &theme, &palette,
                                                    );
                                                }
                                            }
                                            spawn_action_button(
                                                actions,
                                                &theme,
                                                &palette,
                                                "Settings",
                                                TitleAction::OpenSettings,
                                            );
                                            spawn_action_button(
                                                actions,
                                                &theme,
                                                &palette,
                                                "About",
                                                TitleAction::OpenAbout,
                                            );
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
    spawn_themed_button(
        parent,
        theme,
        palette,
        ButtonStyle::Primary,
        Node {
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(18.0), px(14.0)),
            border: UiRect::all(px(1.0)),
            ..default()
        },
        label,
        22.0,
        TitleActionButton { action },
    );
}

/// Like [`spawn_action_button`] but tags the label with [`CleanStateLabel`] so
/// `sync_clean_state_label` can flip it to a confirm prompt. Debug-only.
fn spawn_clean_state_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Primary, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Primary),
            TitleActionButton {
                action: TitleAction::CleanGameState,
            },
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
                Text::new(CLEAN_STATE_IDLE_LABEL),
                CleanStateLabel,
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

const CLEAN_STATE_IDLE_LABEL: &str = "Clean game state";
const CLEAN_STATE_CONFIRM_LABEL: &str = "Click again to wipe + restart";

/// Mirrors the two-click confirm state into the clean-state button's label.
fn sync_clean_state_label(
    title_state: Res<TitleScreenState>,
    mut labels: Query<&mut Text, With<CleanStateLabel>>,
) {
    let desired = if title_state.confirm_clean {
        CLEAN_STATE_CONFIRM_LABEL
    } else {
        CLEAN_STATE_IDLE_LABEL
    };
    for mut text in &mut labels {
        if text.0 != desired {
            text.0 = desired.to_owned();
        }
    }
}

fn handle_title_screen_buttons(
    mut title_state: ResMut<TitleScreenState>,
    mut next_state: ResMut<NextState<ClientAppState>>,
    mut tcp_config: Option<ResMut<TcpClientConfig>>,
    mut tcp_connection: Option<ResMut<TcpClientConnection>>,
    mut pending_auth: Option<ResMut<PendingAuthRequest>>,
    mut exit_messages: MessageWriter<AppExit>,
    mut settings_ui: ResMut<crate::ui::settings::SettingsUiState>,
    world_save: Option<Res<crate::persistence::WorldSaveConfig>>,
    db_path: Option<Res<crate::accounts::AccountDbPath>>,
    action_buttons: Query<(&Interaction, &TitleActionButton), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, button) in &action_buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }

        // Any button other than the clean-state button disarms the confirm.
        if button.action != TitleAction::CleanGameState {
            title_state.confirm_clean = false;
        }

        match button.action {
            TitleAction::Connect => {
                if let Some(tcp_config) = tcp_config.as_mut() {
                    if let Some(server_addr) = title_state
                        .current
                        .as_ref()
                        .and_then(|entry| entry.server_addr.clone())
                    {
                        tcp_config.server_addr = server_addr;
                    }
                    tcp_config.active = true;

                    // Allow `ensure_tcp_client_connected` to attempt the dial
                    // again; without this it sticks at the previous failure.
                    if let Some(connection) = tcp_connection.as_mut() {
                        connection.connect_attempted = false;
                        connection.error_message = None;
                    }

                    if let Some(pending) = pending_auth.as_mut() {
                        if title_state.username.trim().is_empty() {
                            pending.error_message =
                                Some("enter a username before connecting".to_owned());
                            continue;
                        }
                        if title_state.password.is_empty() {
                            pending.error_message =
                                Some("enter a password before connecting".to_owned());
                            continue;
                        }
                        pending.username = title_state.username.clone();
                        pending.password = title_state.password.clone();
                        pending.is_register = title_state.register;
                        pending.sent = false;
                        pending.error_message = None;
                        next_state.set(ClientAppState::Authenticating);
                    } else {
                        // No auth plugin (shouldn't happen in TcpClient mode);
                        // fall through to asset sync.
                        next_state.set(ClientAppState::AssetSync);
                    }
                } else {
                    next_state.set(ClientAppState::CharacterSelect);
                }
            }
            TitleAction::OpenServerPicker => {
                // Embedded mode has nothing to switch between — no-op.
                if title_state.is_tcp() {
                    title_state.modal_open = true;
                }
            }
            TitleAction::ToggleRegister => {
                title_state.register = !title_state.register;
            }
            TitleAction::OpenMapEditor => {
                next_state.set(ClientAppState::MapEditor);
            }
            TitleAction::OpenMapPicker => {
                title_state.map_picker_open = true;
            }
            TitleAction::OpenSettings => {
                settings_ui.toggle();
            }
            TitleAction::OpenAbout => {
                next_state.set(ClientAppState::About);
            }
            TitleAction::CleanGameState => {
                if !title_state.confirm_clean {
                    // First click arms the confirm; the label flips via
                    // `sync_clean_state_label`.
                    title_state.confirm_clean = true;
                    continue;
                }
                // Second click: record the resolved files to wipe and exit.
                // The actual deletion happens pre-boot in `main.rs` (see
                // `clean_cache::consume_wipe_marker`) so it runs while nothing
                // holds the world snapshot or accounts DB open; `main.rs` then
                // re-execs us so the game restarts automatically.
                let mut targets = Vec::new();
                if let Some(world_save) = world_save.as_ref() {
                    targets.push(world_save.path.clone());
                }
                if let Some(db_path) = db_path.as_ref() {
                    if let Some(path) = db_path.0.clone() {
                        targets.push(path);
                    }
                }
                if let Err(err) = crate::app::clean_cache::write_wipe_marker(&targets) {
                    warn!("failed to write clean-state marker: {err}");
                    title_state.confirm_clean = false;
                    continue;
                }
                info!(
                    "clean game state: scheduled wipe of {} file(s), restarting",
                    targets.len()
                );
                exit_messages.write(AppExit::Success);
            }
            TitleAction::Exit => {
                exit_messages.write(AppExit::Success);
            }
        }
    }
}

fn cleanup_title_screen(
    mut commands: Commands,
    mut title_state: ResMut<TitleScreenState>,
    root_query: Query<Entity, With<TitleScreenRoot>>,
    map_modal_query: Query<Entity, With<MapPickerModalRoot>>,
) {
    for entity in &root_query {
        commands.entity(entity).despawn();
    }
    // The map-picker overlay is a sibling root (not a child of TitleScreenRoot),
    // so despawn it explicitly and clear the flag to avoid a leaked overlay if
    // the screen is left with the modal still open.
    for entity in &map_modal_query {
        commands.entity(entity).despawn();
    }
    title_state.map_picker_open = false;
}

fn register_label(register: bool) -> String {
    if register {
        "[X] Register new account".to_owned()
    } else {
        "[ ] Register new account".to_owned()
    }
}

fn field_display(value: &str, is_password: bool) -> String {
    if is_password {
        "•".repeat(value.chars().count())
    } else {
        value.to_owned()
    }
}

/// Shared spawn tree for every title-screen text field (login form and
/// direct-connect modal); only the marker payload and masking differ.
fn spawn_title_field(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    field: TitleField,
    label: &str,
    value: &str,
    focused: bool,
    is_password: bool,
) {
    spawn_text_field(
        parent,
        &TextFieldStyle::title_screen(palette),
        TextFieldVisual {
            label,
            value: &field_display(value, is_password),
            focused,
            show_cursor: false,
            placeholder: "",
            placeholder_color: palette.text_primary,
            value_color: palette.text_primary,
        },
        TitleFieldBox(field),
        TitleFieldText(field),
    );
}

fn handle_title_field_clicks(
    mut title_state: ResMut<TitleScreenState>,
    targets: Query<(&Interaction, &TitleFieldBox), Changed<Interaction>>,
) {
    for (interaction, target) in &targets {
        if *interaction == Interaction::Pressed {
            match target.0 {
                TitleField::Login(field) => title_state.focused = field,
                TitleField::Direct(field) => title_state.direct_focused = Some(field),
            }
        }
    }
}

/// One keyboard pipeline for both field groups. When the server-picker modal
/// is open it owns keyboard focus (the direct-connect inputs claim the events
/// instead of double-typing into both username and host); otherwise the login
/// form does.
fn handle_title_field_keyboard(
    mut title_state: ResMut<TitleScreenState>,
    mut keyboard_events: MessageReader<KeyboardInput>,
) {
    if title_state.modal_open {
        if title_state.direct_focused.is_none() {
            // Nothing to type into; drop our cursor forward so we don't replay
            // backlog the moment a field gets focus.
            keyboard_events.clear();
            return;
        }
        for event in keyboard_events.read() {
            if !event.state.is_pressed() {
                continue;
            }
            let Some(buffer) = direct_field_buffer(&mut title_state) else {
                continue;
            };
            // Filter out control chars; Escape/Enter outcomes are unused here.
            let outcome = apply_text_edit(buffer, event, CharPolicy::PrintableChars);
            if outcome == TextEditOutcome::Next {
                title_state.direct_focused = Some(match title_state.direct_focused {
                    Some(DirectField::Host) => DirectField::Port,
                    _ => DirectField::Host,
                });
            }
        }
    } else {
        if !title_state.is_tcp() {
            return;
        }
        for event in keyboard_events.read() {
            if !event.state.is_pressed() {
                continue;
            }
            // Filter out control chars; tolerate spaces in passwords.
            let outcome = apply_text_edit(
                active_field_buffer(&mut title_state),
                event,
                CharPolicy::PrintableChars,
            );
            if outcome == TextEditOutcome::Next {
                title_state.focused = match title_state.focused {
                    LoginField::Username => LoginField::Password,
                    LoginField::Password => LoginField::Username,
                };
            }
        }
    }
}

fn active_field_buffer(state: &mut TitleScreenState) -> &mut String {
    match state.focused {
        LoginField::Username => &mut state.username,
        LoginField::Password => &mut state.password,
    }
}

fn title_field_display(state: &TitleScreenState, field: TitleField) -> String {
    match field {
        TitleField::Login(LoginField::Username) => field_display(&state.username, false),
        TitleField::Login(LoginField::Password) => field_display(&state.password, true),
        TitleField::Direct(DirectField::Host) => state.direct_host.clone(),
        TitleField::Direct(DirectField::Port) => state.direct_port.clone(),
    }
}

fn title_field_focused(state: &TitleScreenState, field: TitleField) -> bool {
    match field {
        TitleField::Login(f) => state.focused == f,
        TitleField::Direct(f) => state.direct_focused == Some(f),
    }
}

fn sync_title_field_text(
    title_state: Res<TitleScreenState>,
    mut text_query: Query<(&TitleFieldText, &mut Text)>,
    mut register_text: Query<&mut Text, (With<RegisterToggleText>, Without<TitleFieldText>)>,
) {
    for (label, mut text) in &mut text_query {
        let desired = title_field_display(&title_state, label.0);
        if text.0 != desired {
            text.0 = desired;
        }
    }
    for mut text in &mut register_text {
        let desired = register_label(title_state.register);
        if text.0 != desired {
            text.0 = desired;
        }
    }
}

fn sync_title_field_focus_style(
    title_state: Res<TitleScreenState>,
    palette: Res<Palette>,
    mut borders: Query<(&TitleFieldBox, &mut BorderColor)>,
) {
    for (field, mut border) in &mut borders {
        let color = if title_field_focused(&title_state, field.0) {
            palette.border_accent
        } else {
            palette.border_idle
        };
        *border = BorderColor::all(color);
    }
}

fn sync_auth_error_text(
    pending_auth: Option<Res<PendingAuthRequest>>,
    mut error_text: Query<&mut Text, With<AuthErrorText>>,
) {
    let desired = pending_auth
        .as_ref()
        .and_then(|p| p.error_message.clone())
        .unwrap_or_default();
    for mut text in &mut error_text {
        if text.0 != desired {
            text.0 = desired.clone();
        }
    }
}

/// Keeps the title-panel server card's label/description text in sync with
/// the chosen entry. Cheap: only updates the `Text` component when the string
/// differs from the desired value.
fn sync_current_server_card(
    title_state: Res<TitleScreenState>,
    mut texts: Query<(&CurrentServerCardText, &mut Text)>,
) {
    let (label, description) = title_state
        .current
        .as_ref()
        .map(|e| (e.label.clone(), e.description.clone()))
        .unwrap_or_else(|| ("No server selected".to_owned(), String::new()));
    for (kind, mut text) in &mut texts {
        let desired = match kind {
            CurrentServerCardText::Label => &label,
            CurrentServerCardText::Description => &description,
        };
        if text.0 != *desired {
            text.0 = desired.clone();
        }
    }
}

/// Spawn the modal when `modal_open` flips to true; despawn when it flips back.
/// Keeps a single source of truth (`modal_open`) and lets us avoid threading
/// custom events through the system graph.
fn sync_server_picker_modal(
    mut commands: Commands,
    title_state: Res<TitleScreenState>,
    saved: Res<SavedServerList>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    existing: Query<Entity, With<ServerPickerModalRoot>>,
) {
    let want_open = title_state.modal_open && title_state.is_tcp();
    let has_modal = !existing.is_empty();

    if want_open && !has_modal {
        spawn_server_picker_modal(&mut commands, &title_state, &saved, &theme, &palette);
    } else if !want_open && has_modal {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
    }
}

fn spawn_server_picker_modal(
    commands: &mut Commands,
    title_state: &TitleScreenState,
    saved: &SavedServerList,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let entries = build_picker_entries(saved);
    let current_addr = title_state
        .current
        .as_ref()
        .and_then(|e| e.server_addr.clone());

    commands
        .spawn((
            ServerPickerModalRoot,
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.02, 0.02, 0.75)),
            ZIndex(50),
        ))
        .with_children(|root| {
            root.spawn((
                ThemedPanel,
                Node {
                    width: px(480.0),
                    max_width: percent(90.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(14.0),
                    padding: UiRect::all(px(22.0)),
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
                    Text::new("Choose a server"),
                    TextFont {
                        font_size: 26.0,
                        ..default()
                    },
                    TextColor(palette.text_primary),
                ));

                panel.spawn((
                    Text::new("Saved"),
                    TextFont {
                        font_size: 18.0,
                        ..default()
                    },
                    TextColor(palette.text_accent),
                ));

                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8.0),
                        ..default()
                    },))
                    .with_children(|list| {
                        if entries.is_empty() {
                            list.spawn((
                                Text::new("No saved servers."),
                                TextFont {
                                    font_size: 14.0,
                                    ..default()
                                },
                                TextColor(palette.text_muted),
                            ));
                        }
                        for entry in &entries {
                            let selected = entry.server_addr == current_addr;
                            let (bg, border, _) = idle_colors(palette, ButtonStyle::Slot, selected);
                            list.spawn((
                                Button,
                                ThemedButton {
                                    style: ButtonStyle::Slot,
                                    selected,
                                },
                                PickerEntryButton {
                                    entry: entry.clone(),
                                },
                                Node {
                                    width: percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    align_items: AlignItems::Start,
                                    padding: UiRect::all(px(12.0)),
                                    border: UiRect::all(px(1.0)),
                                    row_gap: px(2.0),
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
                                        font_size: 18.0,
                                        ..default()
                                    },
                                    TextColor(palette.text_primary),
                                ));
                                button.spawn((
                                    Text::new(entry.description.clone()),
                                    TextFont {
                                        font_size: 14.0,
                                        ..default()
                                    },
                                    TextColor(palette.text_muted),
                                ));
                            });
                        }
                    });

                panel.spawn((
                    Text::new("Direct connection"),
                    TextFont {
                        font_size: 18.0,
                        ..default()
                    },
                    TextColor(palette.text_accent),
                ));

                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: px(8.0),
                        ..default()
                    },))
                    .with_children(|row| {
                        row.spawn((Node {
                            flex_grow: 1.0,
                            ..default()
                        },))
                            .with_children(|host_col| {
                                spawn_title_field(
                                    host_col,
                                    palette,
                                    TitleField::Direct(DirectField::Host),
                                    "Host",
                                    &title_state.direct_host,
                                    title_state.direct_focused == Some(DirectField::Host),
                                    false,
                                );
                            });
                        row.spawn((Node {
                            width: px(120.0),
                            ..default()
                        },))
                            .with_children(|port_col| {
                                spawn_title_field(
                                    port_col,
                                    palette,
                                    TitleField::Direct(DirectField::Port),
                                    "Port",
                                    &title_state.direct_port,
                                    title_state.direct_focused == Some(DirectField::Port),
                                    false,
                                );
                            });
                    });

                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: px(10.0),
                        ..default()
                    },))
                    .with_children(|footer| {
                        spawn_picker_button(footer, theme, palette, "Cancel", PickerAction::Cancel);
                        spawn_picker_button(
                            footer,
                            theme,
                            palette,
                            "Use this address",
                            PickerAction::UseDirect,
                        );
                    });
            });
        });
}

fn spawn_picker_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    action: PickerAction,
) {
    spawn_themed_button(
        parent,
        theme,
        palette,
        ButtonStyle::Primary,
        Node {
            flex_grow: 1.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(14.0), px(10.0)),
            border: UiRect::all(px(1.0)),
            ..default()
        },
        label,
        18.0,
        action,
    );
}

/// Modal button handler: select a saved entry, build a transient entry from
/// the direct-connect inputs, or cancel out. Picking a saved entry also
/// updates `SavedServerList.selected_addr` so it's restored next launch;
/// direct-connect picks are transient and leave the persisted addr alone.
fn handle_server_picker_buttons(
    mut title_state: ResMut<TitleScreenState>,
    mut saved: ResMut<SavedServerList>,
    entry_buttons: Query<(&Interaction, &PickerEntryButton), (Changed<Interaction>, With<Button>)>,
    action_buttons: Query<(&Interaction, &PickerAction), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, button) in &entry_buttons {
        if *interaction == Interaction::Pressed {
            if let Some(addr) = button.entry.server_addr.as_ref() {
                if saved.selected_addr.as_deref() != Some(addr.as_str()) {
                    saved.selected_addr = Some(addr.clone());
                    saved.dirty = true;
                }
            }
            title_state.current = Some(button.entry.clone());
            title_state.modal_open = false;
            return;
        }
    }

    for (interaction, action) in &action_buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            PickerAction::Cancel => {
                title_state.modal_open = false;
            }
            PickerAction::UseDirect => {
                let host = title_state.direct_host.trim().to_owned();
                let port = title_state.direct_port.trim().to_owned();
                if host.is_empty() || port.is_empty() {
                    continue;
                }
                let addr = format!("{host}:{port}");
                title_state.current = Some(TitleServerEntry {
                    label: format!("Direct: {addr}"),
                    description: format!("Connect to {addr} (session only)."),
                    server_addr: Some(addr),
                });
                title_state.modal_open = false;
            }
        }
    }
}

/// Persistent map ids selectable as a starting map, sorted for a stable list.
fn starting_map_entries(definitions: &SpaceDefinitions) -> Vec<String> {
    let mut ids: Vec<String> = definitions
        .iter()
        .filter(|def| def.permanence.is_persistent())
        .map(|def| def.authored_id.clone())
        .collect();
    ids.sort();
    ids
}

/// Spawn/despawn the starting-map picker modal to track `map_picker_open`.
fn sync_map_picker_modal(
    mut commands: Commands,
    title_state: Res<TitleScreenState>,
    definitions: Res<SpaceDefinitions>,
    selected_map: Res<SelectedStartingMap>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    existing: Query<Entity, With<MapPickerModalRoot>>,
) {
    let want_open = title_state.map_picker_open;
    let has_modal = !existing.is_empty();

    if want_open && !has_modal {
        spawn_map_picker_modal(&mut commands, &definitions, &selected_map, &theme, &palette);
    } else if !want_open && has_modal {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
    }
}

fn spawn_map_picker_modal(
    commands: &mut Commands,
    definitions: &SpaceDefinitions,
    selected_map: &SelectedStartingMap,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let entries = starting_map_entries(definitions);
    // A `None` preference means "use the bootstrap space" — highlight that row.
    let effective_selected = selected_map
        .map_id
        .clone()
        .unwrap_or_else(|| definitions.bootstrap_space_id.clone());

    commands
        .spawn((
            MapPickerModalRoot,
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.02, 0.02, 0.75)),
            ZIndex(50),
        ))
        .with_children(|root| {
            root.spawn((
                ThemedPanel,
                Node {
                    width: px(480.0),
                    max_width: percent(90.0),
                    max_height: percent(85.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(14.0),
                    padding: UiRect::all(px(22.0)),
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
                    Text::new("Starting map"),
                    TextFont {
                        font_size: 26.0,
                        ..default()
                    },
                    TextColor(palette.text_primary),
                ));
                panel.spawn((
                    Text::new("Pick which map a new game spawns into (offline)."),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(palette.text_muted),
                ));

                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },))
                    .with_children(|list| {
                        if entries.is_empty() {
                            list.spawn((
                                Text::new("No maps found."),
                                TextFont {
                                    font_size: 14.0,
                                    ..default()
                                },
                                TextColor(palette.text_muted),
                            ));
                        }
                        for map_id in &entries {
                            let selected = *map_id == effective_selected;
                            let is_bootstrap = *map_id == definitions.bootstrap_space_id;
                            let (bg, border, _) = idle_colors(palette, ButtonStyle::Slot, selected);
                            list.spawn((
                                Button,
                                ThemedButton {
                                    style: ButtonStyle::Slot,
                                    selected,
                                },
                                MapPickerEntryButton {
                                    map_id: map_id.clone(),
                                },
                                Node {
                                    width: percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    align_items: AlignItems::Start,
                                    padding: UiRect::all(px(12.0)),
                                    border: UiRect::all(px(1.0)),
                                    row_gap: px(2.0),
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
                                    Text::new(map_id.clone()),
                                    TextFont {
                                        font_size: 18.0,
                                        ..default()
                                    },
                                    TextColor(palette.text_primary),
                                ));
                                if is_bootstrap {
                                    button.spawn((
                                        Text::new("Default"),
                                        TextFont {
                                            font_size: 14.0,
                                            ..default()
                                        },
                                        TextColor(palette.text_muted),
                                    ));
                                }
                            });
                        }
                    });

                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::End,
                        ..default()
                    },))
                    .with_children(|footer| {
                        footer
                            .spawn((
                                Button,
                                ThemedButton::new(ButtonStyle::Primary),
                                MapPickerCancel,
                                Node {
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    padding: UiRect::axes(px(14.0), px(10.0)),
                                    border: UiRect::all(px(1.0)),
                                    ..default()
                                },
                                ImageNode::new(theme.button_frame.clone())
                                    .with_mode(theme.button_image_mode())
                                    .with_color(
                                        idle_colors(palette, ButtonStyle::Primary, false).0,
                                    ),
                                BackgroundColor(Color::NONE),
                                BorderColor::all(
                                    idle_colors(palette, ButtonStyle::Primary, false).1,
                                ),
                            ))
                            .with_children(|button| {
                                button.spawn((
                                    Text::new("Close"),
                                    TextFont {
                                        font_size: 18.0,
                                        ..default()
                                    },
                                    TextColor(idle_colors(palette, ButtonStyle::Primary, false).2),
                                ));
                            });
                    });
            });
        });
}

/// Map-picker button handler: pick a starting map (persisted via
/// `SelectedStartingMap.dirty`) or close the modal.
fn handle_map_picker_buttons(
    mut title_state: ResMut<TitleScreenState>,
    mut selected_map: ResMut<SelectedStartingMap>,
    entry_buttons: Query<
        (&Interaction, &MapPickerEntryButton),
        (Changed<Interaction>, With<Button>),
    >,
    cancel_buttons: Query<&Interaction, (Changed<Interaction>, With<MapPickerCancel>)>,
) {
    for (interaction, button) in &entry_buttons {
        if *interaction == Interaction::Pressed {
            if selected_map.map_id.as_deref() != Some(button.map_id.as_str()) {
                selected_map.map_id = Some(button.map_id.clone());
                selected_map.dirty = true;
            }
            title_state.map_picker_open = false;
            return;
        }
    }

    for interaction in &cancel_buttons {
        if *interaction == Interaction::Pressed {
            title_state.map_picker_open = false;
            return;
        }
    }
}

fn direct_field_buffer(state: &mut TitleScreenState) -> Option<&mut String> {
    match state.direct_focused? {
        DirectField::Host => Some(&mut state.direct_host),
        DirectField::Port => Some(&mut state.direct_port),
    }
}
