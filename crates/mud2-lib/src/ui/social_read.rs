//! Social-read window: what an NPC thinks of you. The server replies to
//! `GameCommand::RequestSocialRead` (sent by the "Details" context-menu verb
//! on an NPC) with `GameUiEvent::OpenSocialRead`, whose lines are pre-composed
//! server-side — how much they reveal was decided there by a Persuasion
//! check. This module just renders the strings; there is nothing to click but
//! the close button.
//!
//! Same state-resource + `MovableWindow` lifecycle pattern as
//! `ui::crime_ledger`, minus the row buttons.

use bevy::prelude::*;

use crate::ui::movable_window::{
    close_window_and_release_drag, persist_window_geometry, spawn_movable_window,
    spawn_themed_close_button, MovableWindowDrag, MovableWindowId, WindowGeometryMemory,
    MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::theme::{Palette, UiThemeAssets};

const DEFAULT_PANEL_SIZE: Vec2 = Vec2::new(380.0, 240.0);

#[derive(Resource, Default)]
pub struct SocialReadPanelState {
    /// The NPC this read belongs to; `Some` = window open.
    pub npc_object_id: Option<u64>,
    pub npc_name: String,
    /// Pre-formatted read, one paragraph per entry.
    pub lines: Vec<String>,
    /// Bumped on every open/refresh/close so the body rebuild system knows
    /// when to re-render.
    pub revision: u64,
    pub last_position: Option<Vec2>,
    pub last_size: Option<Vec2>,
}

impl SocialReadPanelState {
    pub fn is_open(&self) -> bool {
        self.npc_object_id.is_some()
    }

    /// Open or refresh from a server payload.
    pub fn open(&mut self, npc_object_id: u64, npc_name: String, lines: Vec<String>) {
        self.npc_object_id = Some(npc_object_id);
        self.npc_name = npc_name;
        self.lines = lines;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn close(&mut self) {
        self.npc_object_id = None;
        self.npc_name.clear();
        self.lines.clear();
        self.revision = self.revision.wrapping_add(1);
    }
}

impl WindowGeometryMemory for SocialReadPanelState {
    fn last_position(&self) -> Option<Vec2> {
        self.last_position
    }
    fn last_size(&self) -> Option<Vec2> {
        self.last_size
    }
    fn remember_geometry(&mut self, position: Vec2, size: Vec2) {
        self.last_position = Some(position);
        self.last_size = Some(size);
    }
}

#[derive(Component)]
pub struct SocialReadRoot;

#[derive(Component)]
pub struct SocialReadCloseButton;

#[derive(Resource, Default)]
pub struct SocialReadRenderState {
    pub last_revision: u64,
}

pub fn sync_social_read_lifecycle(
    mut commands: Commands,
    mut state: ResMut<SocialReadPanelState>,
    mut render_state: ResMut<SocialReadRenderState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    existing: Query<(Entity, &Node), With<SocialReadRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let want_open = state.is_open();
    let existing_root = existing.iter().next();

    match (want_open, existing_root) {
        (true, None) => {
            let size = state.last_size.unwrap_or(DEFAULT_PANEL_SIZE);
            let pos = state.last_position.unwrap_or(Vec2::new(420.0, 160.0));
            let spawned = spawn_movable_window(
                &mut commands,
                &theme,
                &palette,
                MovableWindowId::SocialRead,
                &format!("Reading — {}", state.npc_name),
                size,
                pos,
                MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
            );
            commands
                .entity(spawned.root)
                .insert((SocialReadRoot, crate::ui::components::HudRoot));
            commands.entity(spawned.title_bar).with_children(|bar| {
                spawn_themed_close_button(bar, &theme, SocialReadCloseButton);
            });
            drag.focused = Some(spawned.root);
            // Force a body rebuild on the next frame.
            render_state.last_revision = state.revision.wrapping_sub(1);
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

/// Rebuild the read paragraphs whenever the state revision changes (open or
/// reading a different NPC while the window is already up).
pub fn sync_social_read_body(
    mut render_state: ResMut<SocialReadRenderState>,
    state: Res<SocialReadPanelState>,
    palette: Res<Palette>,
    root_query: Query<Entity, With<SocialReadRoot>>,
    body_query: Query<(Entity, &crate::ui::movable_window::MovableWindowContent)>,
    mut commands: Commands,
) {
    if render_state.last_revision == state.revision {
        return;
    }
    let Ok(root) = root_query.single() else {
        return;
    };
    let Some(body_entity) = body_query
        .iter()
        .find(|(_, content)| content.owner == root)
        .map(|(e, _)| e)
    else {
        return;
    };
    render_state.last_revision = state.revision;
    if !state.is_open() {
        return;
    }

    let lines = state.lines.clone();
    let palette_copy = *palette;

    commands.entity(body_entity).despawn_related::<Children>();
    commands.entity(body_entity).with_children(move |body| {
        body.spawn((
            Node {
                width: percent(100.0),
                flex_grow: 1.0,
                min_height: px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(8.0),
                padding: UiRect::all(px(8.0)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(palette_copy.surface_console_output),
        ))
        .with_children(|list| {
            for line in &lines {
                list.spawn((
                    Text::new(line.clone()),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(palette_copy.text_primary),
                ));
            }
        });
    });
}

pub fn handle_social_read_clicks(
    mut state: ResMut<SocialReadPanelState>,
    close_q: Query<&Interaction, (Changed<Interaction>, With<SocialReadCloseButton>)>,
) {
    for interaction in &close_q {
        if *interaction == Interaction::Pressed {
            state.close();
        }
    }
}
