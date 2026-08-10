//! Party panel: [`MountablePanel`] impl plus the roster body, rebuilt from
//! `ClientGameState::party`. Docked in the sidebar it doubles as always-on
//! member health frames; undocked it's the full party screen.

use bevy::prelude::*;

use crate::game::commands::GameCommand;
use crate::game::party::ClientPartyView;
use crate::game::resources::{ClientGameState, ClientPendingCommands};
use crate::ui::components::{
    PartyPanelDockButton, PartyPanelFloatingCloseButton, PartyPanelFloatingRoot,
    PartyPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{DockedPanel, DockedPanelKind, DockedPanelState, PartyPanelMode};
use crate::ui::retro_bar::{spawn_retro_bar, RetroBarStyle};
use crate::ui::systems::hp_fill_color;
use crate::ui::theme::{spawn_themed_button, ButtonStyle, Palette, UiThemeAssets};

pub struct PartyPanel;

impl MountablePanel for PartyPanel {
    type Key = ();
    type Modes = PartyPanelMode;
    type UndockButton = PartyPanelUndockButton;
    type DockButton = PartyPanelDockButton;
    type FloatingRoot = PartyPanelFloatingRoot;
    type FloatingCloseButton = PartyPanelFloatingCloseButton;

    fn movable_window_id(_: ()) -> MovableWindowId {
        MovableWindowId::PartyPanel
    }
    fn floating_size(_: ()) -> Vec2 {
        Vec2::new(300.0, 240.0)
    }
    fn floating_position(_: ()) -> Vec2 {
        Vec2::new(460.0, 200.0)
    }
    fn panel_id_for(_: ()) -> usize {
        DockedPanelState::PARTY_PANEL_ID
    }
    fn active_keys(panel_state: &DockedPanelState) -> Vec<()> {
        if panel_state.is_open(Self::panel_id_for(())) {
            vec![()]
        } else {
            vec![]
        }
    }

    fn docked_definition(_: ()) -> Option<DockedPanel> {
        Some(DockedPanel {
            id: DockedPanelState::PARTY_PANEL_ID,
            kind: DockedPanelKind::Party,
            title: "Party".to_owned(),
            height: DockedPanelState::DEFAULT_PARTY_PANEL_HEIGHT,
            closable: true,
            resizable: true,
            movable: true,
        })
    }

    fn spawn_body(
        parent: &mut ChildSpawnerCommands,
        _: (),
        _theme: &UiThemeAssets,
        palette: &Palette,
        _asset_server: &AssetServer,
    ) {
        spawn_party_panel_body(parent, palette);
    }
}

/// Container node whose children (header + member rows) are rebuilt by
/// [`sync_party_panel`]. One per panel instance (docked and floating).
#[derive(Component)]
pub struct PartyRosterList;

/// Header "Leave" button — any member may leave.
#[derive(Component)]
pub struct PartyLeaveButton;

/// Leader-only per-row "Kick" button.
#[derive(Component)]
pub struct PartyKickButton {
    pub player_id: crate::player::components::PlayerId,
}

/// Leader-only per-row "Lead" (promote) button.
#[derive(Component)]
pub struct PartyPromoteButton {
    pub player_id: crate::player::components::PlayerId,
}

/// Body of the party panel — a scrollable column rebuilt on roster changes.
/// Shared between docked and floating variants via the `MountablePanel` impl.
pub(crate) fn spawn_party_panel_body(parent: &mut ChildSpawnerCommands, palette: &Palette) {
    parent.spawn((
        Node {
            width: percent(100.0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(3.0),
            min_height: px(0.0),
            padding: UiRect::all(px(4.0)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        PartyRosterList,
        ScrollPosition::default(),
        BackgroundColor(palette.surface_raised),
    ));
}

/// Auto-open the docked party panel the moment the local player joins a
/// party. Never auto-closes: a user who closes it mid-session stays closed
/// until the *next* join, and while unpartied the body just renders a
/// placeholder line.
pub fn auto_open_party_panel(
    client_state: Res<ClientGameState>,
    mut docked_panel_state: ResMut<DockedPanelState>,
    mut was_in_party: Local<bool>,
) {
    let in_party = client_state.party.is_some();
    if in_party && !*was_in_party {
        // Immutable probe first — the `&mut self` helper DerefMuts the
        // resource and would defeat `resource_changed` gates downstream.
        if docked_panel_state
            .panel(DockedPanelState::PARTY_PANEL_ID)
            .is_none()
        {
            docked_panel_state.open_party();
        }
    }
    *was_in_party = in_party;
}

/// Rebuilds every `PartyRosterList` instance when the replicated roster (or
/// the set of list instances) changes. Vitals are rounded server-side, so
/// "any change" is per-hit, not per-regen-tick — a full rebuild of ≤6 small
/// rows is cheap at that rate. Never gated on `ClientGameState::is_changed()`
/// (see `character_sheet.rs` for why).
pub fn sync_party_panel(
    mut commands: Commands,
    client_state: Res<ClientGameState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    list_query: Query<Entity, With<PartyRosterList>>,
    mut last: Local<Option<(Option<ClientPartyView>, usize)>>,
) {
    let list_entities: Vec<Entity> = list_query.iter().collect();
    let key = (client_state.party.clone(), list_entities.len());
    if last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);

    let local_id = client_state.local_player_id;
    let is_leader = client_state
        .party
        .as_ref()
        .zip(local_id)
        .is_some_and(|(party, id)| party.is_leader(id));

    for list_entity in list_entities {
        commands.entity(list_entity).despawn_related::<Children>();
        commands.entity(list_entity).with_children(|list| {
            let Some(party) = client_state.party.as_ref() else {
                list.spawn((
                    Text::new("Not in a party."),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(palette.text_muted),
                ));
                return;
            };

            // Header: size + pool bonus on the left, Leave on the right.
            list.spawn((
                Node {
                    width: percent(100.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|header| {
                header.spawn((
                    Text::new(format!(
                        "{} members · +{}% XP pool",
                        party.members.len(),
                        party.xp_bonus_pct()
                    )),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(palette.text_value),
                ));
                spawn_party_button(
                    header,
                    &theme,
                    &palette,
                    ButtonStyle::Secondary,
                    "Leave",
                    PartyLeaveButton,
                );
            });

            for member in &party.members {
                spawn_party_member_row(list, &theme, &palette, member, local_id, is_leader);
            }
        });
    }
}

/// Compact themed button sized for the tight roster rows — same gold frame +
/// hover/press feedback as [`spawn_themed_button`], smaller footprint than
/// `setup::spawn_small_button`.
fn spawn_party_button<T: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    style: ButtonStyle,
    label: &str,
    marker: T,
) {
    spawn_themed_button(
        parent,
        theme,
        palette,
        style,
        Node {
            min_width: px(44.0),
            min_height: px(20.0),
            padding: UiRect::axes(px(6.0), px(2.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1.0)),
            flex_shrink: 0.0,
            ..default()
        },
        label,
        12.0,
        marker,
    );
}

fn spawn_party_member_row(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    member: &crate::game::party::PartyMemberView,
    local_id: Option<crate::player::components::PlayerId>,
    viewer_is_leader: bool,
) {
    // Out-of-range members render dimmed so "why am I not getting XP" is
    // legible straight from the panel.
    let name_color = if member.in_range {
        palette.text_primary
    } else {
        palette.text_muted
    };
    // "»" — the UI font (PixelOperator) is Latin-1 only; no star glyphs.
    let name = if member.is_leader {
        format!("» {}", member.display_name)
    } else {
        member.display_name.clone()
    };
    let hp_ratio = if member.vitals.max_health > 0.0 {
        (member.vitals.health / member.vitals.max_health).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mp_ratio = if member.vitals.max_mana > 0.0 {
        (member.vitals.mana / member.vitals.max_mana).clamp(0.0, 1.0)
    } else {
        0.0
    };

    parent
        .spawn((
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6.0),
                padding: UiRect::all(px(3.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2.0),
                    min_width: px(0.0),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|left| {
                left.spawn((
                    Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(6.0),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|line| {
                    line.spawn((
                        Text::new(name),
                        TextFont {
                            font_size: 13.0,
                            ..default()
                        },
                        TextColor(name_color),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    line.spawn((
                        Text::new(format!(
                            "Lv{} {:?} · {}%",
                            member.level, member.class, member.share_pct
                        )),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(palette.text_muted),
                    ));
                });
                // Same retro pill as the status-panel vitals, scaled down.
                // No fill markers: rows are rebuilt wholesale on any roster
                // change, so the fills never need live width updates.
                spawn_retro_bar(
                    left,
                    palette,
                    RetroBarStyle::default()
                        .with_fill(hp_fill_color(hp_ratio))
                        .with_height(6.0)
                        .with_initial_ratio(hp_ratio),
                    (),
                );
                spawn_retro_bar(
                    left,
                    palette,
                    RetroBarStyle::default()
                        .with_fill(palette.vital_mana_fill)
                        .with_height(4.0)
                        .with_initial_ratio(mp_ratio),
                    (),
                );
            });

            let is_self = local_id == Some(member.player_id);
            if viewer_is_leader && !is_self {
                spawn_party_button(
                    row,
                    theme,
                    palette,
                    ButtonStyle::Danger,
                    "Kick",
                    PartyKickButton {
                        player_id: member.player_id,
                    },
                );
                spawn_party_button(
                    row,
                    theme,
                    palette,
                    ButtonStyle::Secondary,
                    "Lead",
                    PartyPromoteButton {
                        player_id: member.player_id,
                    },
                );
            }
        });
}

/// Leave / Kick / Promote button clicks → wire commands.
pub fn handle_party_panel_buttons(
    leave_query: Query<&Interaction, (Changed<Interaction>, With<PartyLeaveButton>)>,
    kick_query: Query<(&Interaction, &PartyKickButton), Changed<Interaction>>,
    promote_query: Query<(&Interaction, &PartyPromoteButton), Changed<Interaction>>,
    mut pending_commands: ResMut<ClientPendingCommands>,
) {
    for interaction in leave_query.iter() {
        if *interaction == Interaction::Pressed {
            pending_commands.push(GameCommand::LeaveParty);
        }
    }
    for (interaction, kick) in kick_query.iter() {
        if *interaction == Interaction::Pressed {
            pending_commands.push(GameCommand::KickFromParty {
                player_id: kick.player_id,
            });
        }
    }
    for (interaction, promote) in promote_query.iter() {
        if *interaction == Interaction::Pressed {
            pending_commands.push(GameCommand::PromotePartyLeader {
                player_id: promote.player_id,
            });
        }
    }
}
