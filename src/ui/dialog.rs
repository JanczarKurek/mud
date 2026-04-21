//! Dialog panel UI: reads `ActiveDialogState` and drives the overlay spawned
//! by `spawn_dialog_panel`. Click handlers queue `DialogAdvance` /
//! `DialogChoose` / `DialogEnd` game commands.

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;

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
    let Ok(mut visibility) = root_query.single_mut() else {
        return;
    };
    *visibility = if dialog_state.is_active() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
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
    if let Ok(mut speaker) = speaker_query.single_mut() {
        speaker.0 = dialog_state.speaker.clone().unwrap_or_default();
    }
    if let Ok(mut body) = body_query.single_mut() {
        body.0 = dialog_state.text.clone();
    }
}

pub fn sync_dialog_panel_continue_button(
    dialog_state: Res<ActiveDialogState>,
    mut button_query: Query<&mut Node, With<DialogPanelContinueButton>>,
) {
    let Ok(mut node) = button_query.single_mut() else {
        return;
    };
    node.display = if dialog_state.awaiting_continue {
        Display::Flex
    } else {
        Display::None
    };
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

    let Ok(container) = container_query.single() else {
        return;
    };
    commands.entity(container).despawn_related::<Children>();

    let theme = theme.clone();
    let palette = *palette;
    commands.entity(container).with_children(|parent| {
        for (idx, text) in dialog_state.options.iter().enumerate() {
            let (bg, border, text_color) = idle_colors(&palette, ButtonStyle::Secondary, false);
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

/// Route clicks from the dialog panel's buttons (continue, close, options)
/// into game commands. Uses the same `point_in_ui_node` + computed-node
/// inspection pattern as other context-menu click handlers.
pub fn handle_dialog_panel_clicks(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    dialog_state: Res<ActiveDialogState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    continue_query: Query<
        (&ComputedNode, &UiGlobalTransform, &Node),
        With<DialogPanelContinueButton>,
    >,
    close_query: Query<(&ComputedNode, &UiGlobalTransform), With<DialogPanelCloseButton>>,
    option_query: Query<(&DialogPanelOptionButton, &ComputedNode, &UiGlobalTransform)>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(session_id) = dialog_state.session_id else {
        return;
    };
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    if dialog_state.awaiting_continue {
        if let Ok((node, transform, node_style)) = continue_query.single() {
            if node_style.display != Display::None
                && point_in_ui_node(cursor_position, node, transform)
            {
                pending_commands.push(GameCommand::DialogAdvance { session_id });
                return;
            }
        }
    }

    if let Ok((node, transform)) = close_query.single() {
        if point_in_ui_node(cursor_position, node, transform) {
            pending_commands.push(GameCommand::DialogEnd { session_id });
            return;
        }
    }

    for (option, node, transform) in &option_query {
        if point_in_ui_node(cursor_position, node, transform) {
            pending_commands.push(GameCommand::DialogChoose {
                session_id,
                option_idx: option.option_idx,
            });
            return;
        }
    }
}

fn point_in_ui_node(point: Vec2, computed: &ComputedNode, transform: &UiGlobalTransform) -> bool {
    computed.contains_point(*transform, point)
}
