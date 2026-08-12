//! Generic "are you sure?" prompt.
//!
//! One small `MovableWindow` dialog, driven by [`ConfirmPopupState`]: fill in
//! a [`ConfirmRequest`] (title, message, button labels, and the
//! [`ConfirmAction`] to take on yes) and the window appears; either button
//! closes it. The action is an enum rather than a callback because Bevy
//! resources make closures awkward, and every answer in this codebase ends up
//! as a `GameCommand` anyway — the editor's `ModalKind` / `ModalConfirmed`
//! pair works the same way.
//!
//! Adding a new prompt means adding a `ConfirmAction` variant and one
//! `open(...)` call; no new window, systems, or markers.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::game::commands::GameCommand;
use crate::game::resources::ClientPendingCommands;
use crate::game::trade::TradeSessionId;
use crate::ui::movable_window::{
    close_window_and_release_drag, persist_window_geometry, restored_or_centered_geometry,
    spawn_movable_dialog, spawn_themed_close_button, MovableWindowDrag, MovableWindowId,
    WindowGeometryMemory,
};
use crate::ui::resources::{ConfirmAction, ConfirmPopupState};
use crate::ui::setup::spawn_small_button;
use crate::ui::theme::{ButtonStyle, Palette, UiThemeAssets};

/// Marker on the confirm window root.
#[derive(Component)]
pub struct ConfirmWindowRoot;

/// "Yes" — performs the pending [`ConfirmAction`].
#[derive(Component)]
pub struct ConfirmAcceptButton;

/// "No" — drops the request. The title-bar close-X does the same.
#[derive(Component)]
pub struct ConfirmCancelButton;

impl WindowGeometryMemory for ConfirmPopupState {
    fn last_position(&self) -> Option<Vec2> {
        self.last_position
    }
    /// Auto-height dialog — never remember a size (it reads back as 0).
    fn last_size(&self) -> Option<Vec2> {
        None
    }
    fn remember_geometry(&mut self, position: Vec2, _size: Vec2) {
        self.last_position = Some(position);
    }
}

/// Spawn / despawn the window off `ConfirmPopupState.request`.
pub fn sync_confirm_window_lifecycle(
    mut commands: Commands,
    mut state: ResMut<ConfirmPopupState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    existing: Query<(Entity, &Node), With<ConfirmWindowRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let want_open = state.request.is_some();
    let existing_root = existing.iter().next();

    match (want_open, existing_root) {
        (true, None) => {
            let (position, _size) = restored_or_centered_geometry(
                &*state,
                ConfirmPopupState::DEFAULT_SIZE,
                &window_query,
            );
            let root = spawn_confirm_window(&mut commands, &theme, &palette, &state, position);
            drag.focused = Some(root);
        }
        (false, Some((root, _))) => {
            close_window_and_release_drag(&mut commands, &mut drag, root);
        }
        (true, Some((_, node))) => {
            persist_window_geometry(&mut state, node);
        }
        (false, None) => {}
    }
}

fn spawn_confirm_window(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    state: &ConfirmPopupState,
    position: Vec2,
) -> Entity {
    let Some(request) = state.request.as_ref() else {
        unreachable!("spawn_confirm_window called with no pending request");
    };
    let spawned = spawn_movable_dialog(
        commands,
        theme,
        palette,
        MovableWindowId::Confirm,
        &request.title,
        ConfirmPopupState::DEFAULT_SIZE.x,
        position,
    );

    commands
        .entity(spawned.root)
        .insert((ConfirmWindowRoot, crate::ui::components::HudRoot));
    commands.entity(spawned.title_bar).with_children(|bar| {
        spawn_themed_close_button(bar, theme, ConfirmCancelButton);
    });

    let message = request.message.clone();
    let confirm_label = request.confirm_label.clone();
    let cancel_label = request.cancel_label.clone();
    let theme = theme.clone();
    let palette = *palette;
    commands.entity(spawned.body).with_children(move |body| {
        body.spawn((
            Text::new(message),
            TextFont {
                font_size: 13.0,
                ..default()
            },
            TextColor(palette.text_primary),
            Node {
                width: percent(100.0),
                ..default()
            },
        ));
        body.spawn((
            Node {
                width: percent(100.0),
                column_gap: px(6.0),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|buttons| {
            spawn_small_button(
                buttons,
                &theme,
                &palette,
                ButtonStyle::Primary,
                &confirm_label,
                ConfirmAcceptButton,
            );
            spawn_small_button(
                buttons,
                &theme,
                &palette,
                ButtonStyle::Secondary,
                &cancel_label,
                ConfirmCancelButton,
            );
        });
    });

    spawned.root
}

/// Yes / No. Yes turns the pending [`ConfirmAction`] into a command; both
/// close the prompt.
pub fn handle_confirm_popup_buttons(
    accept_query: Query<&Interaction, (Changed<Interaction>, With<ConfirmAcceptButton>)>,
    cancel_query: Query<&Interaction, (Changed<Interaction>, With<ConfirmCancelButton>)>,
    mut state: ResMut<ConfirmPopupState>,
    mut pending_commands: ResMut<ClientPendingCommands>,
) {
    if state.request.is_none() {
        return;
    }
    if cancel_query
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        state.close();
        return;
    }
    if accept_query
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        if let Some(request) = state.request.take() {
            pending_commands.push(command_for(request.action));
        }
    }
}

fn command_for(action: ConfirmAction) -> GameCommand {
    match action {
        ConfirmAction::ConfirmTradeWithDrop { session_id } => {
            GameCommand::ConfirmTradeWithDrop { session_id }
        }
    }
}

/// A prompt about a trade is meaningless once that trade is gone.
pub fn close_confirm_popup_for_trade(state: &mut ConfirmPopupState, session_id: TradeSessionId) {
    let stale = matches!(
        state.request.as_ref().map(|request| request.action),
        Some(ConfirmAction::ConfirmTradeWithDrop { session_id: id }) if id == session_id
    );
    if stale {
        state.close();
    }
}
