//! Dialog panel UI: reads `ActiveDialogState` and drives the overlay spawned
//! by `spawn_dialog_panel`. Click handlers queue `DialogAdvance` /
//! `DialogChoose` / `DialogEnd` game commands.

use bevy::prelude::*;

use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::ui::components::{
    DialogPanelBodyText, DialogPanelCloseButton, DialogPanelContinueButton,
    DialogPanelOptionButton, DialogPanelOptionsContainer, DialogPanelRoot, DialogPanelSpeakerLabel,
};
use crate::ui::resources::ActiveDialogState;
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton};
use crate::ui::theme::{Palette, UiThemeAssets};

/// Ephemeral state so we only rebuild option buttons when the revision
/// actually changes.
#[derive(Resource, Default)]
pub struct DialogPanelRenderState {
    pub last_revision: u64,
}

pub fn sync_dialog_panel_visibility(
    dialog_state: Res<ActiveDialogState>,
    mut root_query: Query<&mut Visibility, With<DialogPanelRoot>>,
) {
    let target = if dialog_state.is_active() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut root_query {
        *visibility = target;
    }
}

pub fn sync_dialog_panel_text(
    dialog_state: Res<ActiveDialogState>,
    mut speaker_query: Query<
        &mut Text,
        (With<DialogPanelSpeakerLabel>, Without<DialogPanelBodyText>),
    >,
    mut body_query: Query<&mut Text, (With<DialogPanelBodyText>, Without<DialogPanelSpeakerLabel>)>,
) {
    if !dialog_state.is_changed() {
        return;
    }
    let speaker_text = dialog_state.speaker.clone().unwrap_or_default();
    for mut speaker in &mut speaker_query {
        speaker.0 = speaker_text.clone();
    }
    for mut body in &mut body_query {
        body.0 = dialog_state.text.clone();
    }
}

pub fn sync_dialog_panel_continue_button(
    dialog_state: Res<ActiveDialogState>,
    mut button_query: Query<&mut Node, With<DialogPanelContinueButton>>,
) {
    let target = if dialog_state.awaiting_continue {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut button_query {
        node.display = target;
    }
}

pub fn sync_dialog_panel_options(
    mut render_state: ResMut<DialogPanelRenderState>,
    dialog_state: Res<ActiveDialogState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    container_query: Query<Entity, With<DialogPanelOptionsContainer>>,
    mut commands: Commands,
) {
    if render_state.last_revision == dialog_state.revision {
        return;
    }
    render_state.last_revision = dialog_state.revision;

    let theme = theme.clone();
    let palette = *palette;
    let options = dialog_state.options.clone();
    for container in &container_query {
        commands.entity(container).despawn_related::<Children>();
        let theme = theme.clone();
        let options = options.clone();
        commands.entity(container).with_children(move |parent| {
            for (idx, text) in options.iter().enumerate() {
                let (bg, border, text_color) =
                    idle_colors(&palette, ButtonStyle::Secondary, false);
                parent
                    .spawn((
                        Button,
                        ThemedButton::new(ButtonStyle::Secondary),
                        DialogPanelOptionButton { option_idx: idx },
                        Node {
                            width: percent(100.0),
                            min_height: px(28.0),
                            padding: UiRect::axes(px(8.0), px(4.0)),
                            align_items: AlignItems::Center,
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
                            Text::new(text.clone()),
                            TextFont {
                                font_size: 16.0,
                                ..default()
                            },
                            TextColor(text_color),
                        ));
                    });
            }
        });
    }
}

/// Route clicks from the dialog panel's buttons (continue, close, options)
/// into game commands. Reads Bevy's per-Button `Interaction::Pressed` state
/// rather than doing manual cursor-vs-rect hit testing — the widgets are
/// already `Button`s, and `Interaction` correctly accounts for layout,
/// transforms, scrolling, scale factor, and `Display::None` without us
/// reimplementing any of it.
pub fn handle_dialog_panel_clicks(
    dialog_state: Res<ActiveDialogState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    continue_query: Query<&Interaction, (Changed<Interaction>, With<DialogPanelContinueButton>)>,
    close_query: Query<&Interaction, (Changed<Interaction>, With<DialogPanelCloseButton>)>,
    option_query: Query<
        (&Interaction, &DialogPanelOptionButton),
        Changed<Interaction>,
    >,
    all_continue: Query<&Interaction, With<DialogPanelContinueButton>>,
    all_close: Query<&Interaction, With<DialogPanelCloseButton>>,
) {
    // Log every Interaction transition we see, even when session_id is None,
    // so we can tell whether the click is reaching us at all and whether the
    // session is the gating issue.
    for interaction in &continue_query {
        bevy::log::info!("dialog_click: continue interaction changed → {:?}", interaction);
    }
    for interaction in &close_query {
        bevy::log::info!("dialog_click: close interaction changed → {:?}", interaction);
    }
    for (interaction, option) in &option_query {
        bevy::log::info!(
            "dialog_click: option {} interaction changed → {:?}",
            option.option_idx, interaction
        );
    }

    let Some(session_id) = dialog_state.session_id else {
        if continue_query.iter().count() > 0
            || close_query.iter().count() > 0
            || option_query.iter().count() > 0
        {
            bevy::log::warn!(
                "dialog_click: interaction fired but session_id is None (continue_widgets={}, close_widgets={})",
                all_continue.iter().count(),
                all_close.iter().count(),
            );
        }
        return;
    };

    if dialog_state.awaiting_continue {
        for interaction in &continue_query {
            if matches!(interaction, Interaction::Pressed) {
                pending_commands.push(GameCommand::DialogAdvance { session_id });
                bevy::log::info!("dialog_click: pushed DialogAdvance (session={session_id})");
                return;
            }
        }
    }

    for interaction in &close_query {
        if matches!(interaction, Interaction::Pressed) {
            pending_commands.push(GameCommand::DialogEnd { session_id });
            bevy::log::info!("dialog_click: pushed DialogEnd (session={session_id})");
            return;
        }
    }

    for (interaction, option) in &option_query {
        if matches!(interaction, Interaction::Pressed) {
            pending_commands.push(GameCommand::DialogChoose {
                session_id,
                option_idx: option.option_idx,
            });
            bevy::log::info!(
                "dialog_click: pushed DialogChoose idx={} (session={session_id})",
                option.option_idx
            );
            return;
        }
    }
}

