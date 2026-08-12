//! The single writer of `stash["codex"]`.
//!
//! Both reveal paths — the active Persuasion read in `npc::social_read` and
//! the passive Perception tick in `npc::bestiary` — hold `iter_mut` borrows of
//! the NPC query while they work, so neither can also take `&mut
//! CharacterStash`. They queue into [`PendingCodexUpdates`] instead and this
//! module applies the lot, following the same deferred-queue idiom as
//! `PendingSocialReadLines` and `npc::guilt::PendingCrimeLearns`.
//!
//! Ordering, all inside `game::CommandIntercept`:
//!
//! ```text
//! process_social_read ────────┐
//! tick_bestiary_observation ──┼─> apply_codex_updates ─> process_log_commands
//! drain_codex_kills ──────────┘
//! ```
//!
//! `apply_codex_updates` pushes `UpsertLogEntry` into `PendingGameCommands` —
//! the *server* queue — because this is server-internal, not client intent.
//! `evaluate_quest_journals` writes quest entries exactly the same way. Because
//! `CommandIntercept` runs ahead of `NetServerSend`, the resulting
//! `LogStateChanged` reaches the client on the same tick as the reveal.

use bevy::prelude::*;

use crate::codex::compose::{compose_bestiary_body, compose_people_body};
use crate::codex::CodexState;
use crate::crafting::CharacterStash;
use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::log::{LogOwner, BESTIARY_SECTION, PEOPLE_SECTION};
use crate::player::components::{Player, PlayerId, PlayerIdentity};
use crate::world::object_definitions::OverworldObjectDefinitions;

/// One piece of knowledge to fold into a player's codex.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodexUpdate {
    /// A Persuasion read reached `tier` on this NPC type.
    NpcTier { definition_id: String, tier: u8 },
    /// An observation roll reached `tier` on this creature type.
    MobTier { definition_id: String, tier: u8 },
    /// One more of this creature type killed. May unlock the mastery tier on
    /// the *next* observation, but never reveals anything on its own.
    Kill { definition_id: String },
}

/// Queue drained by [`apply_codex_updates`].
#[derive(Resource, Default)]
pub struct PendingCodexUpdates {
    pub items: Vec<(PlayerId, CodexUpdate)>,
}

impl PendingCodexUpdates {
    pub fn push(&mut self, player: PlayerId, update: CodexUpdate) {
        self.items.push((player, update));
    }
}

/// Kill credits produced by the damage resolver.
///
/// Separate from [`PendingCodexUpdates`] because its producer lives in
/// `combat`, which must not depend on codex ordering — `drain_codex_kills`
/// bridges the two. Deliberately *not* hung off `QuestEvent::ObjectKilled`:
/// that queue is drained by the feature-gated quest plugin, so kills would go
/// missing in any build without it.
#[derive(Resource, Default)]
pub struct PendingCodexKills {
    pub kills: Vec<(PlayerId, String)>,
}

/// Converts kill credits into codex updates so everything lands through one
/// writer. Runs before [`apply_codex_updates`].
pub fn drain_codex_kills(
    mut kills: ResMut<PendingCodexKills>,
    mut updates: ResMut<PendingCodexUpdates>,
) {
    for (player, definition_id) in kills.kills.drain(..) {
        updates.push(player, CodexUpdate::Kill { definition_id });
    }
}

/// Applies every queued update to the acting player's `stash["codex"]`, and
/// re-composes the log entry for any tier that actually rose.
pub fn apply_codex_updates(
    mut pending: ResMut<PendingCodexUpdates>,
    mut players: Query<(&PlayerIdentity, &mut CharacterStash), With<Player>>,
    mut commands_out: ResMut<PendingGameCommands>,
    definitions: Res<OverworldObjectDefinitions>,
) {
    if pending.items.is_empty() {
        return;
    }
    for (PlayerId(player_id), update) in pending.items.drain(..) {
        let Some((_, mut stash)) = players
            .iter_mut()
            .find(|(identity, _)| identity.id.0 == player_id)
        else {
            // The player disconnected between the reveal and this system.
            continue;
        };

        let mut codex = CodexState::from_stash(&stash);
        let outcome = fold(&mut codex, &update);
        if !outcome.stash_changed {
            continue;
        }
        // A kill changes the stash without changing anything readable, so the
        // write and the log upsert are gated separately: repeated failed rolls
        // must not re-replicate the whole log, but kill tallies must persist
        // or the mastery rung is never reachable.
        codex.write_to_stash(&mut stash);

        let Some((section, definition_id, tier)) = outcome.entry else {
            continue;
        };
        let Some(def) = definitions.get(&definition_id) else {
            warn!("codex: no definition for {definition_id}, entry not written");
            continue;
        };
        let body = match section {
            PEOPLE_SECTION => compose_people_body(&definitions, def, tier),
            _ => compose_bestiary_body(def, tier),
        };
        commands_out.push_for_player(
            PlayerId(player_id),
            GameCommand::UpsertLogEntry {
                section: section.to_owned(),
                subsection: definition_id,
                title: def.name.clone(),
                body,
                owner: LogOwner::Engine,
            },
        );
    }
}

/// What folding one update implies for the two things the caller can write.
#[derive(Debug, Default, PartialEq, Eq)]
struct FoldOutcome {
    /// The stash needs flushing. True for any accepted update, including a
    /// kill, which advances no tier but must still persist.
    stash_changed: bool,
    /// `(section, definition_id, tier)` when a readable tier rose and the log
    /// entry should be re-composed.
    entry: Option<(&'static str, String, u8)>,
}

/// Folds one update into `codex`.
fn fold(codex: &mut CodexState, update: &CodexUpdate) -> FoldOutcome {
    match update {
        CodexUpdate::NpcTier {
            definition_id,
            tier,
        } => match codex.raise_npc_tier(definition_id, *tier) {
            true => FoldOutcome {
                stash_changed: true,
                entry: Some((PEOPLE_SECTION, definition_id.clone(), *tier)),
            },
            false => FoldOutcome::default(),
        },
        CodexUpdate::MobTier {
            definition_id,
            tier,
        } => match codex.raise_mob_tier(definition_id, *tier) {
            true => FoldOutcome {
                stash_changed: true,
                entry: Some((BESTIARY_SECTION, definition_id.clone(), *tier)),
            },
            false => FoldOutcome::default(),
        },
        // Kills are silent: they persist, and may unlock the mastery rung on a
        // later observation, but reveal nothing on their own.
        CodexUpdate::Kill { definition_id } => {
            codex.record_kill(definition_id);
            FoldOutcome {
                stash_changed: true,
                entry: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kill reveals nothing, but it must still reach the stash — otherwise
    /// the tally never grows and the mastery rung is unreachable.
    #[test]
    fn kills_persist_without_writing_the_log() {
        let mut codex = CodexState::default();
        let update = CodexUpdate::Kill {
            definition_id: "wolf".to_owned(),
        };
        let outcome = fold(&mut codex, &update);
        assert!(outcome.stash_changed, "kill tallies must persist");
        assert!(outcome.entry.is_none(), "a kill reveals nothing to read");
        assert_eq!(codex.kills_of("wolf"), 1);
    }

    #[test]
    fn a_repeated_tier_touches_nothing() {
        let mut codex = CodexState::default();
        let update = CodexUpdate::MobTier {
            definition_id: "wolf".to_owned(),
            tier: 2,
        };
        assert!(fold(&mut codex, &update).entry.is_some());

        let repeat = fold(&mut codex, &update);
        assert_eq!(
            repeat,
            FoldOutcome::default(),
            "re-reaching a known tier must not re-replicate the log"
        );
    }

    #[test]
    fn tiers_route_to_their_sections() {
        let mut codex = CodexState::default();
        let (section, id, tier) = fold(
            &mut codex,
            &CodexUpdate::NpcTier {
                definition_id: "villager".to_owned(),
                tier: 3,
            },
        )
        .entry
        .expect("first read should register");
        assert_eq!(section, PEOPLE_SECTION);
        assert_eq!(id, "villager");
        assert_eq!(tier, 3);

        let (section, _, _) = fold(
            &mut codex,
            &CodexUpdate::MobTier {
                definition_id: "rat".to_owned(),
                tier: 1,
            },
        )
        .entry
        .expect("first sighting should register");
        assert_eq!(section, BESTIARY_SECTION);
    }
}
