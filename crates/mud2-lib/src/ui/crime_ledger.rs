//! Crime-ledger window: the per-crime judge fine picker. The server replies
//! to `GameCommand::RequestCrimeList` (sent by the "Pay fine" context-menu
//! verb on a judge) with `GameUiEvent::OpenCrimeLedger`, whose rows are
//! pre-composed server-side ("Murder of Alice" + "2g 4s"); this module just
//! renders them and sends `GameCommand::PayCrime` when a row's Pay button is
//! clicked. Each successful payment makes the server re-send the shrunken
//! ledger — an empty list closes the window (the slate is clean).
//!
//! Same state-resource + `MovableWindow` lifecycle pattern as
//! `ui::book_panel`, with dynamic row rebuilds like `ui::recipe_book`.

use bevy::prelude::*;

use crate::game::commands::GameCommand;
use crate::game::resources::{ClientPendingCommands, CrimeListing};
use crate::ui::movable_window::{
    close_window_and_release_drag, persist_window_geometry, spawn_movable_window,
    spawn_themed_close_button, MovableWindowDrag, MovableWindowId, WindowGeometryMemory,
    MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton};
use crate::ui::theme::{Palette, UiThemeAssets};

const DEFAULT_LEDGER_SIZE: Vec2 = Vec2::new(420.0, 360.0);

#[derive(Resource, Default)]
pub struct CrimeLedgerState {
    /// The judge NPC this ledger belongs to; `Some` = window open.
    pub npc_object_id: Option<u64>,
    pub judge_name: String,
    pub crimes: Vec<CrimeListing>,
    /// Bumped on every open/refresh/close so the body rebuild system knows
    /// when to re-render rows.
    pub revision: u64,
    pub last_position: Option<Vec2>,
    pub last_size: Option<Vec2>,
}

impl CrimeLedgerState {
    pub fn is_open(&self) -> bool {
        self.npc_object_id.is_some()
    }

    /// Open or refresh from a server payload. An empty ledger closes the
    /// window — the server sends one after the last crime is paid off.
    pub fn open(&mut self, npc_object_id: u64, judge_name: String, crimes: Vec<CrimeListing>) {
        if crimes.is_empty() {
            self.close();
            return;
        }
        self.npc_object_id = Some(npc_object_id);
        self.judge_name = judge_name;
        self.crimes = crimes;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn close(&mut self) {
        self.npc_object_id = None;
        self.judge_name.clear();
        self.crimes.clear();
        self.revision = self.revision.wrapping_add(1);
    }
}

impl WindowGeometryMemory for CrimeLedgerState {
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
pub struct CrimeLedgerRoot;

#[derive(Component)]
pub struct CrimeLedgerCloseButton;

/// One row's Pay button. Carries everything the click handler needs so it
/// never has to re-derive which row it sat in.
#[derive(Component)]
pub struct PayCrimeButton {
    pub npc_object_id: u64,
    pub crime_id: u64,
}

#[derive(Resource, Default)]
pub struct CrimeLedgerRenderState {
    pub last_revision: u64,
}

pub fn sync_crime_ledger_lifecycle(
    mut commands: Commands,
    mut state: ResMut<CrimeLedgerState>,
    mut render_state: ResMut<CrimeLedgerRenderState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    existing: Query<(Entity, &Node), With<CrimeLedgerRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let want_open = state.is_open();
    let existing_root = existing.iter().next();

    match (want_open, existing_root) {
        (true, None) => {
            let size = state.last_size.unwrap_or(DEFAULT_LEDGER_SIZE);
            let pos = state.last_position.unwrap_or(Vec2::new(380.0, 140.0));
            let spawned = spawn_movable_window(
                &mut commands,
                &theme,
                &palette,
                MovableWindowId::CrimeLedger,
                &format!("Crimes — {}", state.judge_name),
                size,
                pos,
                MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
            );
            commands
                .entity(spawned.root)
                .insert((CrimeLedgerRoot, crate::ui::components::HudRoot));
            commands.entity(spawned.title_bar).with_children(|bar| {
                spawn_themed_close_button(bar, &theme, CrimeLedgerCloseButton);
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

/// Rebuild the ledger rows whenever the state revision changes (open or
/// server refresh after a payment).
pub fn sync_crime_ledger_body(
    mut render_state: ResMut<CrimeLedgerRenderState>,
    state: Res<CrimeLedgerState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    root_query: Query<Entity, With<CrimeLedgerRoot>>,
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

    let Some(npc_object_id) = state.npc_object_id else {
        return;
    };
    let crimes = state.crimes.clone();
    let theme_owned = theme.clone();
    let palette_copy = *palette;

    commands.entity(body_entity).despawn_related::<Children>();
    commands.entity(body_entity).with_children(move |body| {
        body.spawn((
            Text::new("The magistrate reads out what is held against you:"),
            TextFont {
                font_size: 13.0,
                ..default()
            },
            TextColor(palette_copy.text_value),
            Node {
                margin: UiRect::bottom(px(8.0)),
                ..default()
            },
        ));

        // Scrollable row list.
        body.spawn((
            Node {
                width: percent(100.0),
                flex_grow: 1.0,
                min_height: px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                padding: UiRect::all(px(6.0)),
                border: UiRect::all(px(1.0)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(palette_copy.surface_console_output),
            BorderColor::all(palette_copy.border_accent),
        ))
        .with_children(|list| {
            for crime in &crimes {
                list.spawn(Node {
                    width: percent(100.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: px(8.0),
                    padding: UiRect::axes(px(4.0), px(2.0)),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new(crime.description.clone()),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(palette_copy.text_primary),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    row.spawn((
                        Text::new(crime.price_text.clone()),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(palette_copy.text_value),
                    ));
                    spawn_pay_button(
                        row,
                        &theme_owned,
                        &palette_copy,
                        PayCrimeButton {
                            npc_object_id,
                            crime_id: crime.crime_id,
                        },
                    );
                });
            }
        });
    });
}

fn spawn_pay_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    marker: PayCrimeButton,
) {
    let style = ButtonStyle::Primary;
    let (bg, border, text_color) = idle_colors(palette, style, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(style),
            marker,
            Node {
                min_width: px(52.0),
                min_height: px(24.0),
                padding: UiRect::axes(px(10.0), px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            ImageNode::new(theme.button_frame.clone())
                .with_mode(theme.button_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new("Pay"),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(text_color),
            ));
        });
}

pub fn handle_crime_ledger_clicks(
    mut state: ResMut<CrimeLedgerState>,
    mut pending: ResMut<ClientPendingCommands>,
    pay_q: Query<(&Interaction, &PayCrimeButton), Changed<Interaction>>,
    close_q: Query<&Interaction, (Changed<Interaction>, With<CrimeLedgerCloseButton>)>,
) {
    for (interaction, button) in &pay_q {
        if *interaction == Interaction::Pressed {
            // Optimistic-free: the row stays until the server's refreshed
            // ledger arrives (or a can't-afford narrator line explains why
            // it didn't).
            pending.push(GameCommand::PayCrime {
                npc_object_id: button.npc_object_id,
                crime_id: button.crime_id,
            });
        }
    }
    for interaction in &close_q {
        if *interaction == Interaction::Pressed {
            state.close();
        }
    }
}
