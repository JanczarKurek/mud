//! Parties: grouping, shared XP, and a shared focus target.
//!
//! Parties are ephemeral server state — they live in the [`Parties`] resource
//! only and are never persisted. A party is formed by an invite the target
//! accepts; it disbands the moment it drops below two members, so a player is
//! either alone or in a real group and there is no "party of one" edge case
//! for the UI to render.
//!
//! Mirrors the three-piece shape of `crate::game::trade`: a resource of
//! sessions, a `CommandIntercept` drainer ([`process_party_commands`]), and a
//! per-tick reconciler ([`cleanup_invalid_parties`]) that repairs state after
//! disconnects — `disconnect_peer` despawns the player entity and fires no
//! gameplay hook, so the sweep is what keeps rosters honest.
//!
//! XP sharing does *not* happen here at the kill site. `combat::damage` tags a
//! kill grant with [`XpGrantKind::Kill`] and [`split_party_xp_grants`] rewrites
//! it into per-member grants before `apply_xp_grants` drains the queue, which
//! keeps crafting / admin / scripting XP unsplit.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::resources::ClientVitalStats;
use crate::player::classes::Class;
use crate::player::components::PlayerId;
use crate::world::components::{tile_distance_3d, SpaceId, TilePosition};

#[cfg(feature = "server-sim")]
use crate::game::commands::GameCommand;
#[cfg(feature = "server-sim")]
use crate::game::resources::{ChatLogState, GameUiEvent, PendingGameCommands, PendingGameUiEvents};
#[cfg(feature = "server-sim")]
use crate::player::components::{Player, PlayerIdentity, VitalStats};
#[cfg(feature = "server-sim")]
use crate::player::progression::{Experience, PendingXpGrant, PendingXpGrants, XpGrantKind};
#[cfg(feature = "server-sim")]
use crate::world::components::{OverworldObject, SpaceResident};

pub type PartyId = u64;

/// Hard cap on members. `[tunable]`
pub const MAX_PARTY_SIZE: usize = 5;

/// How close a member must be to a kill to earn a share of it. Matches
/// `projection::INTEREST_RADIUS` so "I can see the fight" and "I get XP for the
/// fight" line up. `[tunable]`
pub const PARTY_SHARE_RADIUS_TILES: i32 = 30;

/// Percent added to the XP pool per member beyond the first. At the
/// [`MAX_PARTY_SIZE`] of 5 the pool is ×1.60. Grouping therefore never costs a
/// full 1/N — it costs `(1 - (1 + 0.15(N-1))/N)`. `[tunable]`
pub const PARTY_XP_BONUS_PCT_PER_EXTRA_MEMBER: u64 = 15;

/// How long an unanswered invite stays live before it is swept. `[tunable]`
pub const PARTY_INVITE_TTL_SECONDS: f32 = 30.0;

// ---------------------------------------------------------------------------
// Wire types (never feature-gated — thin clients share the protocol)
// ---------------------------------------------------------------------------

/// One row of the replicated party roster.
///
/// Deliberately self-contained rather than a pointer into
/// `ClientGameState::remote_players`: that map is same-space + interest-radius
/// filtered and carries no display name, while a party member must stay listed
/// while across the map or in another space entirely.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PartyMemberView {
    pub player_id: PlayerId,
    pub display_name: String,
    pub level: u32,
    pub class: Class,
    /// The member's world object, for click-to-target and marker attachment.
    /// `None` while the member has no live entity (mid-disconnect).
    pub object_id: Option<u64>,
    /// Health/mana rounded to whole points — the projection diffs this, and
    /// raw floats would re-emit the roster on every regen tick.
    pub vitals: ClientVitalStats,
    pub space_id: Option<SpaceId>,
    pub tile: Option<TilePosition>,
    pub online: bool,
    /// Whether this member is currently close enough to the viewer to share
    /// kills. Drives the dimmed row treatment in the party panel.
    pub in_range: bool,
    pub is_leader: bool,
    /// This member's slice of a kill made right now, in percent. `0` when out
    /// of range. Shown per-row so "why did I get so little XP" is answerable
    /// without reading the design doc.
    pub share_pct: u8,
}

/// The local player's view of their party, or `None` when unpartied.
/// Folded into `ClientGameState::party` from `GameEvent::PartyStateChanged`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClientPartyView {
    pub party_id: PartyId,
    pub leader: PlayerId,
    /// Leader first, then join order.
    pub members: Vec<PartyMemberView>,
    /// Object the party has agreed to focus, if any. Every member sees the
    /// same highlight.
    pub focus_target: Option<u64>,
}

impl ClientPartyView {
    /// Whether `player_id` leads this party — the gate for kick/promote/invite
    /// controls in the panel.
    pub fn is_leader(&self, player_id: PlayerId) -> bool {
        self.leader == player_id
    }

    /// Percent bonus currently applied to the shared XP pool, for display.
    pub fn xp_bonus_pct(&self) -> u64 {
        let in_range = self.members.iter().filter(|m| m.in_range).count().max(1);
        PARTY_XP_BONUS_PCT_PER_EXTRA_MEMBER * (in_range as u64 - 1)
    }
}

// ---------------------------------------------------------------------------
// Server-authoritative state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Party {
    pub party_id: PartyId,
    pub leader: PlayerId,
    /// Leader first; `process_party_commands` maintains that invariant.
    pub members: Vec<PlayerId>,
    pub focus_target: Option<u64>,
}

impl Party {
    pub fn contains(&self, player_id: PlayerId) -> bool {
        self.members.contains(&player_id)
    }

    pub fn is_full(&self) -> bool {
        self.members.len() >= MAX_PARTY_SIZE
    }
}

/// A pending invitation. `party_id: None` means the inviter is unpartied and
/// accepting forms a fresh party from the two of them.
#[derive(Clone, Copy, Debug)]
pub struct PartyInvite {
    pub party_id: Option<PartyId>,
    pub from: PlayerId,
    pub to: PlayerId,
    /// `Time::elapsed_secs` at which this invite goes stale.
    pub expires_at: f32,
}

/// What happened to a party when a member was removed — enough for the caller
/// to write the right narrator lines without re-reading the roster.
#[derive(Clone, Debug)]
pub struct PartyDeparture {
    pub party_id: PartyId,
    /// Everyone other than the departing member. On `disbanded` this is the
    /// lone survivor — they still need telling that the party is gone.
    pub remaining: Vec<PlayerId>,
    pub disbanded: bool,
    /// Set when the departure forced a leadership handover.
    pub new_leader: Option<PlayerId>,
}

#[derive(Resource, Default)]
pub struct Parties {
    pub parties: HashMap<PartyId, Party>,
    pub invites: Vec<PartyInvite>,
    /// Display name of every current member, refreshed each tick by
    /// [`cleanup_invalid_parties`]. A disconnect despawns the player entity
    /// before anything notices, so this is the only place left to read a
    /// departing member's name from when writing "X left the party".
    pub last_known_names: HashMap<PlayerId, String>,
    next_id: PartyId,
}

#[cfg_attr(not(feature = "server-sim"), allow(dead_code))]
impl Parties {
    pub fn allocate_id(&mut self) -> PartyId {
        self.next_id += 1;
        self.next_id
    }

    pub fn party_for(&self, player_id: PlayerId) -> Option<&Party> {
        self.parties
            .values()
            .find(|party| party.contains(player_id))
    }

    pub fn party_id_for(&self, player_id: PlayerId) -> Option<PartyId> {
        self.party_for(player_id).map(|party| party.party_id)
    }

    fn party_for_mut(&mut self, player_id: PlayerId) -> Option<&mut Party> {
        self.parties
            .values_mut()
            .find(|party| party.contains(player_id))
    }

    /// Remove `player_id` from whatever party they are in, promoting a new
    /// leader or disbanding as needed. Returns `None` if they weren't partied.
    pub fn remove_member(&mut self, player_id: PlayerId) -> Option<PartyDeparture> {
        let party = self.party_for_mut(player_id)?;
        let party_id = party.party_id;
        party.members.retain(|id| *id != player_id);
        let remaining = party.members.clone();

        // A party of one is just a player, so collapse rather than leaving a
        // degenerate roster the UI would have to special-case.
        if remaining.len() < 2 {
            self.parties.remove(&party_id);
            return Some(PartyDeparture {
                party_id,
                remaining,
                disbanded: true,
                new_leader: None,
            });
        }

        let new_leader = (party.leader == player_id).then(|| {
            let promoted = party.members[0];
            party.leader = promoted;
            promoted
        });
        Some(PartyDeparture {
            party_id,
            remaining,
            disbanded: false,
            new_leader,
        })
    }

    /// Drop every pending invite addressed to `player_id` — used when they
    /// join a party, so competing invites don't linger.
    pub fn clear_invites_to(&mut self, player_id: PlayerId) {
        self.invites.retain(|invite| invite.to != player_id);
    }

    pub fn find_invite(&self, to: PlayerId, from: PlayerId) -> Option<&PartyInvite> {
        self.invites
            .iter()
            .find(|invite| invite.to == to && invite.from == from)
    }
}

// ---------------------------------------------------------------------------
// XP split
// ---------------------------------------------------------------------------

/// Total XP a party of `member_count` earns for a kill worth `base_amount`
/// solo. Integer math throughout so the pool is exactly reproducible.
pub fn party_xp_pool(base_amount: u64, member_count: usize) -> u64 {
    let extra = member_count.saturating_sub(1) as u64;
    base_amount.saturating_mul(100 + PARTY_XP_BONUS_PCT_PER_EXTRA_MEMBER * extra) / 100
}

/// Split a kill's XP across eligible party members, weighted by level.
///
/// Level weighting is what stops power-leveling: a level 1 riding along with
/// level 20s takes home a token share rather than half the kill. Equal-level
/// parties degrade to an even split, which is the common case.
///
/// Uses largest-remainder apportionment, so the returned amounts sum to
/// exactly [`party_xp_pool`] — no XP is invented or lost to rounding.
/// Output order matches `members`.
pub fn split_kill_xp(base_amount: u64, members: &[(PlayerId, u32)]) -> Vec<(PlayerId, u64)> {
    if members.is_empty() {
        return Vec::new();
    }
    let pool = party_xp_pool(base_amount, members.len());

    // Level 0 shouldn't exist, but a zero weight would make a member
    // permanently unpayable and could zero the divisor outright.
    let weights: Vec<u64> = members
        .iter()
        .map(|(_, level)| (*level).max(1) as u64)
        .collect();
    let total_weight: u64 = weights.iter().sum();

    let mut shares: Vec<(PlayerId, u64)> = Vec::with_capacity(members.len());
    let mut remainders: Vec<(u64, usize)> = Vec::with_capacity(members.len());
    let mut assigned: u64 = 0;
    for (index, ((player_id, _), weight)) in members.iter().zip(weights.iter()).enumerate() {
        let numerator = pool.saturating_mul(*weight);
        let floor = numerator / total_weight;
        remainders.push((numerator % total_weight, index));
        assigned += floor;
        shares.push((*player_id, floor));
    }

    // Hand the rounding dust to the largest remainders; ties go to the earlier
    // member so the result is deterministic.
    let mut leftover = pool.saturating_sub(assigned);
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, index) in remainders {
        if leftover == 0 {
            break;
        }
        shares[index].1 += 1;
        leftover -= 1;
    }
    shares
}

/// Each member's percentage of the pool, for the party panel. Rounded to the
/// nearest whole percent independently per member, so the column is readable
/// rather than exactly summing to 100.
pub fn share_percentages(members: &[(PlayerId, u32)]) -> Vec<u8> {
    let total: u64 = members
        .iter()
        .map(|(_, level)| (*level).max(1) as u64)
        .sum();
    if total == 0 {
        return vec![0; members.len()];
    }
    members
        .iter()
        .map(|(_, level)| {
            let weight = (*level).max(1) as u64;
            ((weight * 200 + total) / (total * 2)).min(100) as u8
        })
        .collect()
}

/// Whether `member_tile` is close enough to a kill at `kill_tile` to share it.
/// Same space plus a Chebyshev window — the same shape the chat radius and the
/// remote-player interest filter use.
pub fn within_share_range(
    kill_space: SpaceId,
    kill_tile: TilePosition,
    member_space: SpaceId,
    member_tile: TilePosition,
) -> bool {
    kill_space == member_space
        && tile_distance_3d(kill_tile, member_tile) <= PARTY_SHARE_RADIUS_TILES
}

// ---------------------------------------------------------------------------
// Server systems
// ---------------------------------------------------------------------------

/// Identity snapshot taken once per drain so handlers can resolve
/// `object_id → PlayerId → name` without re-borrowing the player query while
/// they hold a mutable borrow of a chat log.
#[cfg(feature = "server-sim")]
#[derive(Clone)]
struct RosterRow {
    player_id: PlayerId,
    object_id: u64,
    display_name: String,
}

#[cfg(feature = "server-sim")]
type PartyPlayerQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PlayerIdentity,
        &'static OverworldObject,
        &'static mut ChatLogState,
    ),
    With<Player>,
>;

#[cfg(feature = "server-sim")]
fn narrate(query: &mut PartyPlayerQuery, player_id: PlayerId, message: impl Into<String>) {
    if let Some((_, _, mut chat_log)) = query
        .iter_mut()
        .find(|(identity, _, _)| identity.id == player_id)
    {
        chat_log.push_narrator(message);
    }
}

#[cfg(feature = "server-sim")]
fn name_of(roster: &[RosterRow], player_id: PlayerId) -> String {
    roster
        .iter()
        .find(|row| row.player_id == player_id)
        .map(|row| row.display_name.clone())
        .unwrap_or_else(|| format!("Player#{}", player_id.0))
}

/// Drains every `Party*` command. Registered `.in_set(CommandIntercept)` so the
/// variants never reach `process_game_commands` — mirrors
/// `process_trade_commands`.
#[cfg(feature = "server-sim")]
pub fn process_party_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut parties: ResMut<Parties>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    time: Res<Time>,
    mut player_query: PartyPlayerQuery,
) {
    let claimed = pending_commands.drain_matching(|command| match command {
        claimed @ (GameCommand::InviteToParty { .. }
        | GameCommand::AcceptPartyInvite { .. }
        | GameCommand::DeclinePartyInvite { .. }
        | GameCommand::LeaveParty
        | GameCommand::KickFromParty { .. }
        | GameCommand::PromotePartyLeader { .. }
        | GameCommand::SetPartyFocusTarget { .. }) => Ok(claimed),
        other => Err(other),
    });
    if claimed.is_empty() {
        return;
    }

    let roster: Vec<RosterRow> = player_query
        .iter()
        .map(|(identity, object, _)| RosterRow {
            player_id: identity.id,
            object_id: object.object_id,
            display_name: identity.display_name.clone(),
        })
        .collect();
    let now = time.elapsed_secs();

    for (queued_player_id, command) in claimed {
        // `None` is the server-internal path (scripts / admin REPL); embedded
        // and TCP clients both arrive peer-attributed.
        let acting = match queued_player_id {
            Some(id) => id,
            None => match roster.first() {
                Some(row) => row.player_id,
                None => continue,
            },
        };

        match command {
            GameCommand::InviteToParty { target_object_id } => handle_invite(
                acting,
                target_object_id,
                now,
                &roster,
                &mut parties,
                &mut ui_events,
                &mut player_query,
            ),
            GameCommand::AcceptPartyInvite { from } => handle_accept(
                acting,
                from,
                now,
                &roster,
                &mut parties,
                &mut ui_events,
                &mut player_query,
            ),
            GameCommand::DeclinePartyInvite { from } => handle_decline(
                acting,
                from,
                &roster,
                &mut parties,
                &mut ui_events,
                &mut player_query,
            ),
            GameCommand::LeaveParty => {
                handle_departure(acting, acting, &roster, &mut parties, &mut player_query)
            }
            GameCommand::KickFromParty { player_id } => {
                handle_kick(acting, player_id, &roster, &mut parties, &mut player_query)
            }
            GameCommand::PromotePartyLeader { player_id } => {
                handle_promote(acting, player_id, &roster, &mut parties, &mut player_query)
            }
            GameCommand::SetPartyFocusTarget { object_id } => {
                if let Some(party) = parties.party_for_mut(acting) {
                    party.focus_target = object_id;
                }
            }
            // The matcher above only claims the party variants.
            _ => {}
        }
    }
}

#[cfg(feature = "server-sim")]
#[allow(clippy::too_many_arguments)]
fn handle_invite(
    acting: PlayerId,
    target_object_id: u64,
    now: f32,
    roster: &[RosterRow],
    parties: &mut Parties,
    ui_events: &mut PendingGameUiEvents,
    player_query: &mut PartyPlayerQuery,
) {
    let Some(target) = roster
        .iter()
        .find(|row| row.object_id == target_object_id)
        .cloned()
    else {
        // Not a player object. A correct client never offers the verb here,
        // so stay silent on the wire and just log it.
        crate::game::helpers::refuse(acting, "InviteToParty", "target is not a player");
        return;
    };
    if target.player_id == acting {
        return;
    }

    if parties.party_id_for(target.player_id).is_some() {
        narrate(
            player_query,
            acting,
            format!("{} is already in a party.", target.display_name),
        );
        return;
    }

    // Unpartied inviters implicitly form a party of two on accept; partied
    // ones must be the leader and have room.
    let party_id = parties.party_id_for(acting);
    let party_size = match party_id {
        Some(id) => {
            let party = &parties.parties[&id];
            if party.leader != acting {
                narrate(
                    player_query,
                    acting,
                    "Only the party leader can invite new members.",
                );
                return;
            }
            if party.is_full() {
                narrate(
                    player_query,
                    acting,
                    format!("Your party is full ({MAX_PARTY_SIZE} members)."),
                );
                return;
            }
            party.members.len()
        }
        None => 1,
    };

    if parties.find_invite(target.player_id, acting).is_some() {
        narrate(
            player_query,
            acting,
            format!("{} already has your invitation.", target.display_name),
        );
        return;
    }

    parties.invites.push(PartyInvite {
        party_id,
        from: acting,
        to: target.player_id,
        expires_at: now + PARTY_INVITE_TTL_SECONDS,
    });
    ui_events.push(
        target.player_id,
        GameUiEvent::PartyInviteReceived {
            from_player_id: acting,
            from_name: name_of(roster, acting),
            party_size,
        },
    );
    narrate(
        player_query,
        acting,
        format!("You invite {} to your party.", target.display_name),
    );
}

#[cfg(feature = "server-sim")]
#[allow(clippy::too_many_arguments)]
fn handle_accept(
    acting: PlayerId,
    from: PlayerId,
    now: f32,
    roster: &[RosterRow],
    parties: &mut Parties,
    ui_events: &mut PendingGameUiEvents,
    player_query: &mut PartyPlayerQuery,
) {
    let Some(invite) = parties.find_invite(acting, from).copied() else {
        narrate(
            player_query,
            acting,
            "That party invitation is no longer available.",
        );
        ui_events.push(acting, GameUiEvent::PartyInviteClosed);
        return;
    };
    parties
        .invites
        .retain(|entry| !(entry.to == acting && entry.from == from));
    ui_events.push(acting, GameUiEvent::PartyInviteClosed);

    if invite.expires_at <= now {
        narrate(player_query, acting, "That party invitation has expired.");
        return;
    }
    if parties.party_id_for(acting).is_some() {
        narrate(player_query, acting, "You are already in a party.");
        return;
    }

    let members = match invite.party_id {
        Some(party_id) => {
            let Some(party) = parties.parties.get_mut(&party_id) else {
                narrate(player_query, acting, "That party no longer exists.");
                return;
            };
            if party.is_full() {
                narrate(player_query, acting, "That party is full.");
                return;
            }
            party.members.push(acting);
            party.members.clone()
        }
        None => {
            // The inviter may have joined someone else while this sat pending.
            if parties.party_id_for(from).is_some() {
                narrate(
                    player_query,
                    acting,
                    format!("{} is already in a party.", name_of(roster, from)),
                );
                return;
            }
            if !roster.iter().any(|row| row.player_id == from) {
                narrate(player_query, acting, "That player is no longer online.");
                return;
            }
            let party_id = parties.allocate_id();
            let members = vec![from, acting];
            parties.parties.insert(
                party_id,
                Party {
                    party_id,
                    leader: from,
                    members: members.clone(),
                    focus_target: None,
                },
            );
            members
        }
    };

    // Competing invitations are void the moment they'd be redundant.
    parties.clear_invites_to(acting);

    let joined_name = name_of(roster, acting);
    for member in members {
        if member == acting {
            continue;
        }
        narrate(
            player_query,
            member,
            format!("{joined_name} joined the party."),
        );
    }
    narrate(player_query, acting, "You joined the party.");
}

#[cfg(feature = "server-sim")]
fn handle_decline(
    acting: PlayerId,
    from: PlayerId,
    roster: &[RosterRow],
    parties: &mut Parties,
    ui_events: &mut PendingGameUiEvents,
    player_query: &mut PartyPlayerQuery,
) {
    let had_invite = parties.find_invite(acting, from).is_some();
    parties
        .invites
        .retain(|entry| !(entry.to == acting && entry.from == from));
    ui_events.push(acting, GameUiEvent::PartyInviteClosed);
    if had_invite {
        narrate(
            player_query,
            from,
            format!(
                "{} declined your party invitation.",
                name_of(roster, acting)
            ),
        );
    }
}

/// Shared tail of Leave and Kick: remove `leaving` and tell everyone involved.
/// `actor` is who pressed the button, used only to word the message.
#[cfg(feature = "server-sim")]
fn handle_departure(
    actor: PlayerId,
    leaving: PlayerId,
    roster: &[RosterRow],
    parties: &mut Parties,
    player_query: &mut PartyPlayerQuery,
) {
    let Some(departure) = parties.remove_member(leaving) else {
        return;
    };
    let leaving_name = name_of(roster, leaving);
    let kicked = actor != leaving;

    if kicked {
        narrate(player_query, leaving, "You were removed from the party.");
    } else {
        narrate(player_query, leaving, "You left the party.");
    }

    if departure.disbanded {
        for member in &departure.remaining {
            narrate(player_query, *member, "The party has disbanded.");
        }
        return;
    }

    for member in &departure.remaining {
        let line = if kicked {
            format!("{leaving_name} was removed from the party.")
        } else {
            format!("{leaving_name} left the party.")
        };
        narrate(player_query, *member, line);
    }
    if let Some(new_leader) = departure.new_leader {
        let leader_name = name_of(roster, new_leader);
        for member in &departure.remaining {
            narrate(
                player_query,
                *member,
                format!("{leader_name} now leads the party."),
            );
        }
    }
}

#[cfg(feature = "server-sim")]
fn handle_kick(
    acting: PlayerId,
    target: PlayerId,
    roster: &[RosterRow],
    parties: &mut Parties,
    player_query: &mut PartyPlayerQuery,
) {
    let Some(party) = parties.party_for(acting) else {
        return;
    };
    if party.leader != acting {
        narrate(
            player_query,
            acting,
            "Only the party leader can remove members.",
        );
        return;
    }
    if target == acting || !party.contains(target) {
        return;
    }
    handle_departure(acting, target, roster, parties, player_query);
}

#[cfg(feature = "server-sim")]
fn handle_promote(
    acting: PlayerId,
    target: PlayerId,
    roster: &[RosterRow],
    parties: &mut Parties,
    player_query: &mut PartyPlayerQuery,
) {
    let members = {
        let Some(party) = parties.party_for_mut(acting) else {
            return;
        };
        if party.leader != acting || target == acting || !party.contains(target) {
            return;
        }
        party.leader = target;
        // Keep the leader-first invariant so every roster reads the same order.
        party.members.retain(|id| *id != target);
        party.members.insert(0, target);
        party.members.clone()
    };
    let leader_name = name_of(roster, target);
    for member in members {
        narrate(
            player_query,
            member,
            format!("{leader_name} now leads the party."),
        );
    }
}

/// Per-tick reconciler: expires invites, evicts members whose entity is gone,
/// and clears focus targets that no longer exist.
///
/// `disconnect_peer` only queues a save and despawns; no gameplay subsystem is
/// notified. This sweep is therefore the only thing keeping party state
/// consistent across disconnects, crashes, and deaths that despawn an entity —
/// the same role `cleanup_invalid_trades` plays for trades.
#[cfg(feature = "server-sim")]
pub fn cleanup_invalid_parties(
    mut parties: ResMut<Parties>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    time: Res<Time>,
    mut player_query: PartyPlayerQuery,
    object_query: Query<&OverworldObject>,
) {
    let now = time.elapsed_secs();
    if parties.parties.is_empty() && parties.invites.is_empty() {
        return;
    }

    let roster: Vec<RosterRow> = player_query
        .iter()
        .map(|(identity, object, _)| RosterRow {
            player_id: identity.id,
            object_id: object.object_id,
            display_name: identity.display_name.clone(),
        })
        .collect();
    let is_live = |id: PlayerId| roster.iter().any(|row| row.player_id == id);

    // Refresh the name cache while every member is still resolvable.
    for row in &roster {
        if parties.party_id_for(row.player_id).is_some() {
            parties
                .last_known_names
                .insert(row.player_id, row.display_name.clone());
        }
    }

    // Expired / orphaned invitations.
    let mut closed_for: Vec<PlayerId> = Vec::new();
    parties.invites.retain(|invite| {
        let alive = invite.expires_at > now && is_live(invite.from) && is_live(invite.to);
        if !alive {
            closed_for.push(invite.to);
        }
        alive
    });
    for player_id in closed_for {
        ui_events.push(player_id, GameUiEvent::PartyInviteClosed);
    }

    // Members whose entity vanished (disconnect, despawn).
    let departed: Vec<PlayerId> = parties
        .parties
        .values()
        .flat_map(|party| party.members.iter().copied())
        .filter(|id| !is_live(*id))
        .collect();
    for player_id in departed {
        let Some(departure) = parties.remove_member(player_id) else {
            continue;
        };
        // The departed player's own entity is already gone, so only the
        // survivors can be told anything — and only the cache still knows
        // what to call them.
        let gone_name = parties
            .last_known_names
            .remove(&player_id)
            .unwrap_or_else(|| format!("Player#{}", player_id.0));
        for member in &departure.remaining {
            narrate(
                &mut player_query,
                *member,
                format!("{gone_name} left the party (disconnected)."),
            );
        }
        if departure.disbanded {
            for member in &departure.remaining {
                narrate(&mut player_query, *member, "The party has disbanded.");
            }
            continue;
        }
        if let Some(new_leader) = departure.new_leader {
            let leader_name = name_of(&roster, new_leader);
            for member in &departure.remaining {
                narrate(
                    &mut player_query,
                    *member,
                    format!("{leader_name} now leads the party."),
                );
            }
        }
    }

    // Focus targets that despawned (killed mob, picked-up item).
    let needs_focus_check = parties
        .parties
        .values()
        .any(|party| party.focus_target.is_some());
    if needs_focus_check {
        let live_objects: std::collections::HashSet<u64> =
            object_query.iter().map(|object| object.object_id).collect();
        for party in parties.parties.values_mut() {
            if let Some(object_id) = party.focus_target {
                if !live_objects.contains(&object_id) {
                    party.focus_target = None;
                }
            }
        }
    }

    // Don't let the name cache outlive the parties it exists for.
    let partied: std::collections::HashSet<PlayerId> = parties
        .parties
        .values()
        .flat_map(|party| party.members.iter().copied())
        .collect();
    parties
        .last_known_names
        .retain(|player_id, _| partied.contains(player_id));
}

/// Rewrite kill-sourced XP grants into per-member shares.
///
/// Ordered between `apply_pending_damage` (which tags the grant) and
/// `apply_xp_grants` (which banks it). Crafting, admin, and scripting grants
/// carry [`XpGrantKind::Direct`] and pass through untouched — that is the whole
/// reason the split lives here rather than inside `apply_xp_grants`.
#[cfg(feature = "server-sim")]
pub fn split_party_xp_grants(
    mut grants: ResMut<PendingXpGrants>,
    parties: Res<Parties>,
    mut player_query: Query<
        (
            &PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &VitalStats,
            &Experience,
            &mut ChatLogState,
        ),
        With<Player>,
    >,
) {
    if grants.grants.is_empty() || parties.parties.is_empty() {
        return;
    }

    let queued = std::mem::take(&mut grants.grants);
    let mut out: Vec<PendingXpGrant> = Vec::with_capacity(queued.len());
    // Narrator lines for partied recipients, pushed after the rewrite loop so
    // the loop doesn't fight the query over the chat-log borrow.
    let mut share_lines: Vec<(PlayerId, String)> = Vec::new();
    for grant in queued {
        let XpGrantKind::Kill { space_id, tile } = grant.kind else {
            out.push(grant);
            continue;
        };
        let Some(party) = parties.party_for(grant.player_id) else {
            out.push(PendingXpGrant::direct(grant.player_id, grant.amount));
            continue;
        };

        // Eligible = alive, same space, and close enough to the kill. The
        // killer always qualifies even when the blow landed remotely (a trap
        // or a summon can kill from outside the radius).
        let eligible: Vec<(PlayerId, u32)> = party
            .members
            .iter()
            .filter_map(|member| {
                let (_, resident, member_tile, vitals, experience, _) = player_query
                    .iter()
                    .find(|(identity, _, _, _, _, _)| identity.id == *member)?;
                let qualifies = *member == grant.player_id
                    || (vitals.health > 0.0
                        && within_share_range(space_id, tile, resident.space_id, *member_tile));
                qualifies.then_some((*member, experience.level))
            })
            .collect();

        // The killer's own "[X gained N XP]" broadcast is suppressed in
        // `apply_pending_damage` for partied killers, so every partied
        // recipient — split or pass-through — gets a narrator line here.
        if eligible.len() < 2 {
            out.push(PendingXpGrant::direct(grant.player_id, grant.amount));
            share_lines.push((
                grant.player_id,
                format!("You gain {} XP from the kill.", grant.amount),
            ));
            continue;
        }
        for (player_id, amount) in split_kill_xp(grant.amount, &eligible) {
            out.push(PendingXpGrant::direct(player_id, amount));
            share_lines.push((
                player_id,
                format!("You gain {amount} XP from the party's kill."),
            ));
        }
    }
    grants.grants = out;

    for (player_id, line) in share_lines {
        if let Some((_, _, _, _, _, mut chat_log)) = player_query
            .iter_mut()
            .find(|(identity, _, _, _, _, _)| identity.id == player_id)
        {
            chat_log.push_narrator(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(members: &[(u64, u32)]) -> Vec<(PlayerId, u32)> {
        members
            .iter()
            .map(|(id, level)| (PlayerId(*id), *level))
            .collect()
    }

    #[test]
    fn pool_grows_with_party_size() {
        assert_eq!(party_xp_pool(750, 1), 750);
        assert_eq!(party_xp_pool(750, 2), 862); // x1.15
        assert_eq!(party_xp_pool(750, 3), 975); // x1.30
        assert_eq!(party_xp_pool(750, 5), 1_200); // x1.60
    }

    #[test]
    fn shares_sum_exactly_to_the_pool() {
        for base in [1u64, 7, 75, 750, 1_500, 999_999] {
            for party in [
                ids(&[(1, 3)]),
                ids(&[(1, 3), (2, 3)]),
                ids(&[(1, 20), (2, 10), (3, 1)]),
                ids(&[(1, 7), (2, 7), (3, 7), (4, 7), (5, 7)]),
                ids(&[(1, 1), (2, 2), (3, 3), (4, 19), (5, 20)]),
            ] {
                let pool = party_xp_pool(base, party.len());
                let total: u64 = split_kill_xp(base, &party).iter().map(|(_, xp)| xp).sum();
                assert_eq!(total, pool, "base {base}, party of {}", party.len());
            }
        }
    }

    #[test]
    fn equal_levels_split_evenly() {
        let shares = split_kill_xp(750, &ids(&[(1, 5), (2, 5), (3, 5)]));
        // Pool 975 over three equal members = 325 each, no dust.
        assert_eq!(
            shares,
            vec![(PlayerId(1), 325), (PlayerId(2), 325), (PlayerId(3), 325)]
        );
    }

    #[test]
    fn low_level_leech_earns_a_token_share() {
        // The worked example from the design discussion: L20 + L10 + L1.
        let shares = split_kill_xp(750, &ids(&[(1, 20), (2, 10), (3, 1)]));
        assert_eq!(shares[0].1, 629);
        assert_eq!(shares[1].1, 315);
        assert_eq!(shares[2].1, 31);
        // The leech takes well under an even third.
        assert!(shares[2].1 < 975 / 3);
    }

    #[test]
    fn solo_member_keeps_the_full_amount() {
        assert_eq!(
            split_kill_xp(750, &ids(&[(1, 12)])),
            vec![(PlayerId(1), 750)]
        );
        assert!(split_kill_xp(750, &[]).is_empty());
    }

    #[test]
    fn zero_level_members_do_not_divide_by_zero() {
        let shares = split_kill_xp(100, &ids(&[(1, 0), (2, 0)]));
        assert_eq!(shares.iter().map(|(_, xp)| xp).sum::<u64>(), 115);
    }

    #[test]
    fn share_percentages_track_levels() {
        assert_eq!(share_percentages(&ids(&[(1, 5), (2, 5)])), vec![50, 50]);
        assert_eq!(
            share_percentages(&ids(&[(1, 20), (2, 10), (3, 1)])),
            vec![65, 32, 3]
        );
        assert_eq!(share_percentages(&[]), Vec::<u8>::new());
    }

    #[test]
    fn removing_a_member_promotes_or_disbands() {
        let mut parties = Parties::default();
        let party_id = parties.allocate_id();
        parties.parties.insert(
            party_id,
            Party {
                party_id,
                leader: PlayerId(1),
                members: vec![PlayerId(1), PlayerId(2), PlayerId(3)],
                focus_target: None,
            },
        );

        // Leader leaves: the next member is promoted, party survives.
        let departure = parties.remove_member(PlayerId(1)).expect("was partied");
        assert!(!departure.disbanded);
        assert_eq!(departure.new_leader, Some(PlayerId(2)));
        assert_eq!(parties.parties[&party_id].leader, PlayerId(2));

        // Dropping to one member collapses the party entirely.
        let departure = parties.remove_member(PlayerId(3)).expect("was partied");
        assert!(departure.disbanded);
        assert!(parties.parties.is_empty());
        assert!(parties.party_for(PlayerId(2)).is_none());
    }

    #[test]
    fn share_range_is_same_space_and_bounded() {
        let origin = TilePosition::ground(10, 10);
        assert!(within_share_range(
            SpaceId(0),
            origin,
            SpaceId(0),
            TilePosition::ground(30, 10)
        ));
        assert!(!within_share_range(
            SpaceId(0),
            origin,
            SpaceId(0),
            TilePosition::ground(41, 10)
        ));
        assert!(!within_share_range(
            SpaceId(0),
            origin,
            SpaceId(1),
            TilePosition::ground(10, 10)
        ));
    }
}
