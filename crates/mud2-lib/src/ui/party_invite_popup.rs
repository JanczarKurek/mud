//! Party-invite prompt: a small `MovableWindow` driven by
//! [`PartyInvitePopupState`] (fed by `GameUiEvent::PartyInviteReceived`,
//! cleared by `PartyInviteClosed`). Accept, Decline, and the close-X all
//! answer the server — the stock close-X only despawns locally, which would
//! leave the server holding a live invite, so this window wires its own.

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::game::commands::GameCommand;
use crate::game::resources::ClientPendingCommands;
use crate::ui::hit_test::point_in_ui_node;
use crate::ui::movable_window::{
    close_window_and_release_drag, persist_window_geometry, restored_or_centered_geometry,
    spawn_movable_dialog, spawn_themed_close_button, MovableWindowDrag, MovableWindowId,
    WindowGeometryMemory,
};
use crate::ui::resources::PartyInvitePopupState;
use crate::ui::setup::spawn_small_button;
use crate::ui::theme::{ButtonStyle, Palette, UiThemeAssets};

/// Marker on the invite window root.
#[derive(Component)]
pub struct PartyInviteWindowRoot;

/// Accept button — answers the invite affirmatively.
#[derive(Component)]
pub struct PartyInviteAcceptButton;

/// Decline button. The title-bar close-X carries the same marker semantics
/// via [`PartyInviteCloseButton`]; both send `DeclinePartyInvite`.
#[derive(Component)]
pub struct PartyInviteDeclineButton;

/// Title-bar close-X — declines rather than despawning locally.
#[derive(Component)]
pub struct PartyInviteCloseButton;

impl WindowGeometryMemory for PartyInvitePopupState {
    fn last_position(&self) -> Option<Vec2> {
        self.last_position
    }
    fn last_size(&self) -> Option<Vec2> {
        None
    }
    fn remember_geometry(&mut self, position: Vec2, _size: Vec2) {
        self.last_position = Some(position);
    }
}

/// Spawn / despawn the invite window off `PartyInvitePopupState.invite`.
pub fn sync_party_invite_window_lifecycle(
    mut commands: Commands,
    mut state: ResMut<PartyInvitePopupState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    existing: Query<(Entity, &Node), With<PartyInviteWindowRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let want_open = state.invite.is_some();
    let existing_root = existing.iter().next();

    match (want_open, existing_root) {
        (true, None) => {
            // DEFAULT_SIZE is only a centering estimate — the dialog itself
            // is auto-height at DEFAULT_SIZE.x wide.
            let (pos, _size) = restored_or_centered_geometry(
                &*state,
                PartyInvitePopupState::DEFAULT_SIZE,
                &window_query,
            );
            let root = spawn_invite_window(&mut commands, &theme, &palette, &state, pos);
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

fn spawn_invite_window(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    state: &PartyInvitePopupState,
    position: Vec2,
) -> Entity {
    let Some(invite) = state.invite.as_ref() else {
        unreachable!("spawn_invite_window called with no pending invite");
    };
    let spawned = spawn_movable_dialog(
        commands,
        theme,
        palette,
        MovableWindowId::PartyInvite,
        "Party Invitation",
        PartyInvitePopupState::DEFAULT_SIZE.x,
        position,
    );

    commands
        .entity(spawned.root)
        .insert((PartyInviteWindowRoot, crate::ui::components::HudRoot));
    commands.entity(spawned.title_bar).with_children(|bar| {
        spawn_themed_close_button(bar, theme, PartyInviteCloseButton);
    });

    let line = if invite.party_size <= 1 {
        format!("{} invites you to form a party.", invite.from_name)
    } else {
        format!(
            "{} invites you to their party ({} members).",
            invite.from_name, invite.party_size
        )
    };
    commands.entity(spawned.body).with_children(|body| {
        body.spawn((
            Text::new(line),
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
                theme,
                palette,
                ButtonStyle::Primary,
                "Accept",
                PartyInviteAcceptButton,
            );
            spawn_small_button(
                buttons,
                theme,
                palette,
                ButtonStyle::Secondary,
                "Decline",
                PartyInviteDeclineButton,
            );
        });
    });

    spawned.root
}

/// Accept / Decline button clicks. The local popup closes when the server
/// echoes `PartyInviteClosed`, not here — keeps the window honest about
/// what the server actually did with the answer.
pub fn handle_party_invite_buttons(
    accept_query: Query<&Interaction, (Changed<Interaction>, With<PartyInviteAcceptButton>)>,
    decline_query: Query<&Interaction, (Changed<Interaction>, With<PartyInviteDeclineButton>)>,
    state: Res<PartyInvitePopupState>,
    mut pending_commands: ResMut<ClientPendingCommands>,
) {
    let Some(invite) = state.invite.as_ref() else {
        return;
    };
    for interaction in accept_query.iter() {
        if *interaction == Interaction::Pressed {
            pending_commands.push(GameCommand::AcceptPartyInvite {
                from: invite.from_player_id,
            });
        }
    }
    for interaction in decline_query.iter() {
        if *interaction == Interaction::Pressed {
            pending_commands.push(GameCommand::DeclinePartyInvite {
                from: invite.from_player_id,
            });
        }
    }
}

/// The title-bar close-X sends a decline; despawn follows via the lifecycle
/// once the server confirms with `PartyInviteClosed`.
pub fn handle_party_invite_close_click(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    state: Res<PartyInvitePopupState>,
    mut pending_commands: ResMut<ClientPendingCommands>,
    button_query: Query<(&ComputedNode, &UiGlobalTransform), With<PartyInviteCloseButton>>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(invite) = state.invite.as_ref() else {
        return;
    };
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((node, transform)) = button_query.single() else {
        return;
    };
    if point_in_ui_node(cursor, node, transform) {
        pending_commands.push(GameCommand::DeclinePartyInvite {
            from: invite.from_player_id,
        });
    }
}
