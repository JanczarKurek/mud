//! Social reads: a Persuasion-gated glimpse of who an NPC is and what they
//! think of you.
//!
//! Inspecting an NPC (`GameCommand::Inspect`) implicitly rolls a Persuasion
//! check against the NPC's demeanor and drops a one-line summary into chat;
//! the "Details" context-menu verb (`GameCommand::RequestSocialRead`) replies
//! with the full dossier as `GameUiEvent::OpenSocialRead`. How much the read
//! reveals scales with the check's margin:
//!
//! | Margin | Tier | Adds |
//! |---|---|---|
//! | (failure) | 0 | name, occupation, description — you can still see them |
//! | ≥ 0 | 1 | their bearing toward you |
//! | ≥ [`MARGIN_CRIMES`] | 1 | whether they know of your crimes, and how bad |
//! | ≥ [`MARGIN_FACTIONS`] | 2 | which factions' grudges they carry |
//! | ≥ [`MARGIN_LORE`] | 3 | background lore and their social ties |
//!
//! The durable half of a successful read (who they are, allegiances, lore) is
//! also filed into the reader's People codex via `codex::PendingCodexUpdates`,
//! so the Log window keeps what the popup showed. The live half (bearing,
//! crime knowledge) is deliberately *not* filed — it changes, and a stale
//! "they hate you" line in a journal would lie.
//!
//! Each (player, NPC) pair rolls at most once per [`SOCIAL_READ_COOLDOWN_SECS`]
//! — inside the window both paths replay the cached read verbatim, so the
//! check can't be spam-rerolled. The cache is session-only state, like
//! `world::hidden::Hidden`'s check schedule: it deliberately does not survive
//! a world reload.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::game::commands::{GameCommand, InspectTarget};
use crate::game::resources::{DossierBearing, DossierRelation, NpcDossier};
use crate::npc::components::{Faction, HostileBehavior, Npc};
use crate::npc::guilt::{CrimeMemory, FactionInterner, GuiltTier};
use crate::npc::hostility::{is_hostile_toward, Aggressor, Subject, TagMask, TagProfile};
use crate::player::components::{BaseStats, ChatLog, Player, PlayerId, PlayerIdentity};
use crate::player::skills::{skill_check, Skill, SkillSheet};
use crate::world::components::OverworldObject;
use crate::world::components::{SpaceResident, TilePosition};

/// Seconds a (player, NPC) social read stays cached — and therefore how long
/// until the pair is eligible for a fresh roll. `[tunable]`
pub const SOCIAL_READ_COOLDOWN_SECS: f64 = 60.0;

/// Base Persuasion DC for reading someone. `[tunable]`
pub const SOCIAL_READ_BASE_DC: i32 = 12;

/// Margin at which the read also reveals the NPC's knowledge of your crimes.
pub const MARGIN_CRIMES: i32 = 5;

/// Margin at which the read also names the factions whose grudges it carries.
pub const MARGIN_FACTIONS: i32 = 10;

/// Margin at which the read also yields background lore and the NPC's ties to
/// other people and factions.
pub const MARGIN_LORE: i32 = 15;

/// One resolved read, replayed verbatim until it expires.
#[derive(Clone, Debug)]
pub struct CachedSocialRead {
    /// Absolute `Time::elapsed_secs_f64()` past which a new roll is allowed.
    pub expires_at: f64,
    /// The one-line chat summary.
    pub summary: String,
    /// The full dossier. Cached *structured*, not pre-formatted: caching
    /// rendered strings would replay stale text the moment a new field is
    /// added to the window.
    pub dossier: NpcDossier,
}

/// Per-NPC cache of social reads keyed by reader. Seeded on every fresh NPC by
/// `resolve_npc_tag_components` (so the command handler's query always
/// matches) and never persisted — a reload forgets who sized whom up.
#[derive(Component, Clone, Debug, Default)]
pub struct SocialReadMemory {
    pub reads: HashMap<PlayerId, CachedSocialRead>,
}

impl SocialReadMemory {
    /// The still-valid cached read for `player`, if any.
    pub fn fresh(&self, player: PlayerId, now: f64) -> Option<&CachedSocialRead> {
        self.reads
            .get(&player)
            .filter(|cached| now < cached.expires_at)
    }

    pub fn store(&mut self, player: PlayerId, now: f64, summary: String, dossier: NpcDossier) {
        self.reads.insert(
            player,
            CachedSocialRead {
                expires_at: now + SOCIAL_READ_COOLDOWN_SECS,
                summary,
                dossier,
            },
        );
    }
}

/// Chat summaries produced by this frame's social reads, held back so they
/// land *after* `process_game_commands` has pushed the inspect description.
/// Drained by [`emit_social_read_lines`].
#[derive(Resource, Default)]
pub struct PendingSocialReadLines {
    pub items: Vec<(PlayerId, String)>,
}

/// How the NPC regards the reader, coarsest first.
///
/// Aliased to the ungated wire type in `game::resources`: this module is
/// `server-sim`-gated, and the attitude has to reach the thin client.
pub use crate::game::resources::DossierAttitude as Attitude;

/// Fold the hostility predicate, the guilt ledger, and faction alignment into
/// one word. `hostile` is the same per-viewer verdict the projection's red
/// marker uses, so the read never contradicts what the client already renders.
pub fn derive_attitude(hostile: bool, guilt_points: u32, faction: Faction) -> Attitude {
    if hostile {
        return Attitude::Hostile;
    }
    if GuiltTier::of(guilt_points) >= GuiltTier::Shunned {
        return Attitude::Wary;
    }
    if faction == Faction::PlayerSide && guilt_points == 0 {
        return Attitude::Friendly;
    }
    Attitude::Neutral
}

fn attitude_phrase(attitude: Attitude, npc_name: &str) -> String {
    match attitude {
        Attitude::Friendly => format!("{npc_name} regards you warmly."),
        Attitude::Neutral => format!("{npc_name} pays you little mind."),
        Attitude::Wary => format!("{npc_name} eyes you with cold distrust."),
        Attitude::Hostile => format!("{npc_name} looks ready to kill you."),
    }
}

/// Highest dossier tier a read can reach (bearing → crimes → factions → lore).
pub const MAX_DOSSIER_TIER: u8 = 4;

/// The dossier tier a read of `margin` unlocks: 1 for a bare success, up to
/// [`MAX_DOSSIER_TIER`].
pub fn tier_for_margin(margin: i32) -> u8 {
    1 + u8::from(margin >= MARGIN_CRIMES)
        + u8::from(margin >= MARGIN_FACTIONS)
        + u8::from(margin >= MARGIN_LORE)
}

/// Maps a dossier tier onto the People codex's three-rung ladder.
///
/// They differ because the codex only records *durable* knowledge: the crimes
/// rung (dossier tier 2) is live state and leaves no trace in the journal, so
/// tiers 1 and 2 both file as codex tier 1.
pub fn codex_tier_for_dossier(dossier_tier: u8) -> u8 {
    match dossier_tier {
        0 => 0,
        1 | 2 => 1,
        3 => 2,
        _ => 3,
    }
}

/// Everything a dossier needs that comes from the NPC's *definition* rather
/// than from the roll. Grouped so `compose_dossier` doesn't take ten strings.
pub struct DossierSubject<'a> {
    pub name: &'a str,
    pub occupation: Option<&'a str>,
    pub description: &'a str,
    pub lore: Option<&'a str>,
    /// Faction display names this NPC answers to.
    pub factions: Vec<String>,
    /// `(note, subject)` social ties, from `codex::compose::derive_relationships`.
    pub relationships: Vec<(String, String)>,
}

impl DossierSubject<'_> {
    /// The tier-0 dossier: who they are, and nothing else. The floor for both
    /// a success and a failure — you can always see a person.
    fn identity(&self) -> NpcDossier {
        NpcDossier {
            name: self.name.to_owned(),
            occupation: self.occupation.map(str::to_owned),
            description: self.description.to_owned(),
            ..Default::default()
        }
    }
}

/// The successful read's chat summary + dossier, tiered by margin.
pub fn compose_dossier(
    subject: &DossierSubject<'_>,
    attitude: Attitude,
    margin: i32,
    guilt_points: u32,
    grudge_factions: &[String],
) -> (String, NpcDossier) {
    let summary = attitude_phrase(attitude, subject.name);

    let crime_note = (margin >= MARGIN_CRIMES).then(|| {
        if guilt_points == 0 {
            "Your name means nothing ill to them.".to_owned()
        } else {
            match GuiltTier::of(guilt_points) {
                GuiltTier::Clean => {
                    "They know of a misdeed of yours, but think it minor.".to_owned()
                }
                GuiltTier::Shunned => {
                    "They know enough of your crimes to want nothing to do with you.".to_owned()
                }
                GuiltTier::Wanted => "They know your crimes — enough to want you dead.".to_owned(),
            }
        }
    });

    // The faction tier reports two different things at once: whose colours
    // they wear, and whose grudge they carry. Both are worth the margin.
    let mut factions = Vec::new();
    if margin >= MARGIN_FACTIONS {
        factions.extend(subject.factions.iter().cloned());
        for grudge in grudge_factions {
            if !factions.contains(grudge) {
                factions.push(grudge.clone());
            }
        }
    }

    let mut dossier = NpcDossier {
        bearing: Some(DossierBearing {
            attitude,
            phrase: summary.clone(),
            crime_note,
        }),
        factions,
        tier: tier_for_margin(margin),
        ..subject.identity()
    };

    if margin >= MARGIN_LORE {
        dossier.lore = subject.lore.map(str::to_owned);
        dossier.relationships = subject
            .relationships
            .iter()
            .map(|(note, subject)| DossierRelation {
                note: note.clone(),
                subject: subject.clone(),
            })
            .collect();
    }

    (summary, dossier)
}

/// The failed read: you can see who they are, and learn nothing more.
pub fn compose_failure(subject: &DossierSubject<'_>) -> (String, NpcDossier) {
    (
        format!("{} gives nothing away.", subject.name),
        NpcDossier {
            failed: true,
            ..subject.identity()
        },
    )
}

/// Roll (or replay) `player`'s read of one NPC. Returns the summary, the
/// dossier, and whether this call performed a fresh roll.
#[allow(clippy::too_many_arguments)]
fn ensure_read(
    memory: &mut SocialReadMemory,
    now: f64,
    player: PlayerId,
    sheet: &SkillSheet,
    attributes: &crate::player::components::AttributeSet,
    subject: &DossierSubject<'_>,
    npc_faction: Faction,
    hostile: bool,
    guilt: Option<&CrimeMemory>,
    interner: &FactionInterner,
) -> (String, NpcDossier, bool) {
    if let Some(cached) = memory.fresh(player, now) {
        return (cached.summary.clone(), cached.dossier.clone(), false);
    }

    let guilt_points = guilt.map(|g| g.points(player)).unwrap_or(0);
    let attitude = derive_attitude(hostile, guilt_points, npc_faction);
    let mut dc = crate::player::check::Dc::new(SOCIAL_READ_BASE_DC, "read intent");
    if attitude == Attitude::Hostile {
        dc.add(3, "on edge");
    }
    if GuiltTier::of(guilt_points) >= GuiltTier::Shunned {
        dc.add(2, "distrusts you");
    }
    let result = skill_check(sheet, attributes, Skill::Persuasion, dc.total(), 0);

    let (summary, mut dossier) = if result.success {
        // Which factions' grudges this NPC carries against the reader — the
        // union of the victim factions across its records of the reader.
        let grudge_mask = guilt
            .map(|g| {
                g.records
                    .iter()
                    .filter(|r| r.player == player)
                    .fold(TagMask::EMPTY, |acc, r| {
                        TagMask(acc.0 | interner.resolve(&r.victim_factions).0)
                    })
            })
            .unwrap_or(TagMask::EMPTY);
        let grudge_factions = interner.display_names(grudge_mask);
        compose_dossier(
            subject,
            attitude,
            result.total - dc.total(),
            guilt_points,
            &grudge_factions,
        )
    } else {
        compose_failure(subject)
    };
    dossier.check_line = format!("(Persuasion {} vs DC {})", result.total, dc.explain());

    memory.store(player, now, summary.clone(), dossier.clone());
    (summary, dossier, true)
}

type SocialReadPlayerQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PlayerIdentity,
        &'static SpaceResident,
        &'static TilePosition,
        &'static BaseStats,
        &'static SkillSheet,
    ),
    With<Player>,
>;

type SocialReadNpcQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static OverworldObject,
        &'static SpaceResident,
        &'static TilePosition,
        Option<&'static Faction>,
        Option<&'static TagProfile>,
        Option<&'static CrimeMemory>,
        Has<HostileBehavior>,
        &'static mut SocialReadMemory,
    ),
    With<Npc>,
>;

/// Handles the two entry points into a social read, in the `CommandIntercept`
/// set (like `npc::guilt::process_judge_commands`):
///
/// - **Peeks** at queued `Inspect { Object }` commands *without draining them*
///   (`handle_inspect` still owns the description) — an NPC target in talk
///   range gets a read whose summary is queued into
///   [`PendingSocialReadLines`], emitted after the inspect text.
/// - **Drains** `RequestSocialRead` (the Details verb) and replies with
///   `GameUiEvent::OpenSocialRead`.
#[allow(clippy::too_many_arguments)]
pub fn process_social_read(
    mut pending_commands: ResMut<crate::game::resources::PendingGameCommands>,
    time: Res<Time>,
    players: SocialReadPlayerQuery,
    mut npcs: SocialReadNpcQuery,
    definitions: Res<crate::world::object_definitions::OverworldObjectDefinitions>,
    spell_definitions: Res<crate::magic::resources::SpellDefinitions>,
    object_registry: Res<crate::world::object_registry::ObjectRegistry>,
    floors: crate::world::column::FloorGeometryParam,
    interner: Res<FactionInterner>,
    mut ui_events: ResMut<crate::game::resources::PendingGameUiEvents>,
    mut chat_lines: ResMut<PendingSocialReadLines>,
    mut codex_updates: ResMut<crate::codex::PendingCodexUpdates>,
) {
    // Cheap read-only probe first, like `process_judge_commands`: don't touch
    // (and change-flag) anything for the overwhelmingly common empty frame.
    let has_ours = pending_commands.commands.iter().any(|queued| {
        matches!(
            queued.command,
            GameCommand::RequestSocialRead { .. }
                | GameCommand::Inspect {
                    target: InspectTarget::Object(_),
                }
        )
    });
    if !has_ours {
        return;
    }

    // The implicit inspect reads, collected before the drain reshuffles the
    // queue. Deduped so a double-queued inspect doesn't echo the summary.
    let mut inspect_reads: Vec<(PlayerId, u64)> = Vec::new();
    for queued in pending_commands.commands.iter() {
        if let (
            Some(player_id),
            GameCommand::Inspect {
                target: InspectTarget::Object(object_id),
            },
        ) = (queued.player_id, &queued.command)
        {
            if !inspect_reads.contains(&(player_id, *object_id)) {
                inspect_reads.push((player_id, *object_id));
            }
        }
    }

    let explicit = pending_commands.drain_matching(|command| match command {
        GameCommand::RequestSocialRead { npc_object_id } => Ok(npc_object_id),
        other => Err(other),
    });

    let geometry = floors.geometry();
    let now = time.elapsed_secs_f64();

    // (player, npc, explicit?) — inspects first so an inspect+Details pair in
    // one frame rolls once and the window replays the same read.
    let requests = inspect_reads
        .iter()
        .map(|(player, object)| (*player, *object, false))
        .chain(
            explicit
                .into_iter()
                .filter_map(|(player, object)| player.map(|p| (p, object, true))),
        );

    for (player_id, npc_object_id, is_explicit) in requests {
        let Some((
            npc_object,
            npc_resident,
            npc_tile,
            faction,
            tags,
            guilt,
            has_hostile,
            mut memory,
        )) = npcs
            .iter_mut()
            .find(|(object, ..)| object.object_id == npc_object_id)
        else {
            if is_explicit {
                crate::game::helpers::refuse(player_id, "SocialRead", "not an NPC");
            }
            continue;
        };
        let Some((_, resident, tile, base_stats, sheet)) = players
            .iter()
            .find(|(identity, ..)| identity.id == player_id)
        else {
            continue;
        };
        // Reading someone is a close-up act: same reach rule as Talk. A plain
        // inspect from farther out still gets its description, just no read.
        if resident.space_id != npc_resident.space_id
            || !geometry.talk_reachable(tile, npc_tile, npc_resident.space_id)
        {
            if is_explicit {
                crate::game::helpers::refuse(player_id, "SocialRead", "NPC out of range");
            }
            continue;
        }

        // Same per-viewer hostility verdict as the projection's red marker,
        // including its MonsterSide fallback for legacy faction-less hostiles.
        // Socially, though, a faction-less NPC (shopkeeper, quest-giver) reads
        // as the PlayerSide default — see the `Faction` doc.
        let social_faction = faction.copied().unwrap_or_default();
        let hostile = has_hostile
            && is_hostile_toward(
                Aggressor::new(
                    faction.copied().unwrap_or(Faction::MonsterSide),
                    tags.map(|t| t.hostile_towards).unwrap_or_default(),
                    guilt,
                ),
                Subject::new(Faction::PlayerSide, TagMask::PLAYER, Some(player_id)),
            );
        let npc_name = object_registry
            .display_name(npc_object.object_id, &definitions, &spell_definitions)
            .unwrap_or_else(|| "They".to_owned());

        // Definition-sourced half of the dossier. A missing definition (an
        // NPC spawned from a deleted template) still yields a usable read —
        // just one with nothing but a name.
        let def = definitions.get(&npc_object.definition_id);
        let subject = DossierSubject {
            name: &npc_name,
            occupation: def.and_then(|d| d.occupation.as_deref()),
            description: def.map(|d| d.description_for_count(1)).unwrap_or(""),
            lore: def.and_then(|d| d.lore.as_deref()),
            factions: def
                .map(|d| {
                    d.factions
                        .iter()
                        .map(|f| definitions.faction_display_name(f))
                        .collect()
                })
                .unwrap_or_default(),
            relationships: def
                .map(|d| crate::codex::compose::derive_relationships(&definitions, d))
                .unwrap_or_default(),
        };

        let (summary, dossier, fresh) = ensure_read(
            &mut memory,
            now,
            player_id,
            sheet,
            &base_stats.attributes,
            &subject,
            social_faction,
            hostile,
            guilt,
            &interner,
        );

        // File the durable half into the reader's People codex. Only on a
        // fresh roll: a cache replay learned nothing new.
        if fresh && dossier.tier > 0 && def.is_some() {
            codex_updates.push(
                player_id,
                crate::codex::CodexUpdate::NpcTier {
                    definition_id: npc_object.definition_id.clone(),
                    tier: codex_tier_for_dossier(dossier.tier),
                },
            );
        }

        if is_explicit {
            ui_events.push(
                player_id,
                crate::game::resources::GameUiEvent::OpenSocialRead {
                    npc_object_id,
                    npc_name,
                    dossier,
                },
            );
            // The window shows everything; only a fresh roll also narrates.
            if fresh {
                chat_lines.items.push((player_id, summary));
            }
        } else {
            chat_lines.items.push((player_id, summary));
        }
    }
}

/// Drains [`PendingSocialReadLines`] into chat logs. Registered after
/// `process_game_commands` so the summary lands *below* the inspect
/// description it annotates.
pub fn emit_social_read_lines(
    mut pending: ResMut<PendingSocialReadLines>,
    mut players: Query<(&PlayerIdentity, &mut ChatLog)>,
) {
    if pending.items.is_empty() {
        return;
    }
    for (player_id, line) in pending.items.drain(..) {
        if let Some((_, mut chat)) = players
            .iter_mut()
            .find(|(identity, _)| identity.id == player_id)
        {
            chat.push_narrator(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::components::{SpaceId, SpaceResident, TilePosition};

    // ------------------------------------------------------------------
    // Pure logic
    // ------------------------------------------------------------------

    #[test]
    fn attitude_matrix() {
        // Hostility trumps everything.
        assert_eq!(
            derive_attitude(true, 0, Faction::PlayerSide),
            Attitude::Hostile
        );
        // A Shunned-tier grudge makes even a player-sider wary.
        assert_eq!(
            derive_attitude(
                false,
                crate::npc::guilt::SHUNNED_THRESHOLD,
                Faction::PlayerSide
            ),
            Attitude::Wary
        );
        // Clean player-sider is friendly; a known (sub-Shunned) misdeed
        // drops that to neutral.
        assert_eq!(
            derive_attitude(false, 0, Faction::PlayerSide),
            Attitude::Friendly
        );
        assert_eq!(
            derive_attitude(false, 10, Faction::PlayerSide),
            Attitude::Neutral
        );
        assert_eq!(
            derive_attitude(false, 0, Faction::Neutral),
            Attitude::Neutral
        );
    }

    fn test_subject() -> DossierSubject<'static> {
        DossierSubject {
            name: "Guard",
            occupation: Some("Watch Sergeant"),
            description: "Broad-shouldered and bored.",
            lore: Some("Took the post after the mill fire."),
            factions: vec!["The Emberbrook Watch".to_owned()],
            relationships: vec![("Protects".to_owned(), "Emberbrook".to_owned())],
        }
    }

    #[test]
    fn compose_dossier_tiers_by_margin() {
        let subject = test_subject();
        let grudges = vec!["Emberbrook Watch".to_owned()];

        // Bare success: bearing, nothing deeper.
        let (_, bare) = compose_dossier(&subject, Attitude::Wary, 0, 40, &grudges);
        assert_eq!(bare.tier, 1);
        assert!(bare.bearing.is_some());
        assert!(bare.bearing.as_ref().unwrap().crime_note.is_none());
        assert!(bare.factions.is_empty());
        assert!(bare.lore.is_none());

        // +5: crime knowledge.
        let (_, crimes) = compose_dossier(&subject, Attitude::Wary, MARGIN_CRIMES, 40, &grudges);
        assert_eq!(crimes.tier, 2);
        let note = crimes.bearing.unwrap().crime_note.expect("crime note");
        assert!(note.contains("nothing to do with you"), "{note}");

        // +10: allegiances, including the grudge-bearing faction.
        let (_, full) = compose_dossier(&subject, Attitude::Wary, MARGIN_FACTIONS, 40, &grudges);
        assert_eq!(full.tier, 3);
        assert!(full.factions.contains(&"The Emberbrook Watch".to_owned()));
        assert!(full.lore.is_none(), "lore is a tier above factions");

        // +15: background and social ties.
        let (_, deep) = compose_dossier(&subject, Attitude::Wary, MARGIN_LORE, 40, &grudges);
        assert_eq!(deep.tier, MAX_DOSSIER_TIER);
        assert_eq!(
            deep.lore.as_deref(),
            Some("Took the post after the mill fire.")
        );
        assert_eq!(deep.relationships.len(), 1);
        assert_eq!(deep.relationships[0].note, "Protects");

        // The codex ladder collapses the live crimes rung: tiers 1 and 2 are
        // the same durable knowledge.
        assert_eq!(codex_tier_for_dossier(1), 1);
        assert_eq!(codex_tier_for_dossier(2), 1);
        assert_eq!(codex_tier_for_dossier(3), 2);
        assert_eq!(codex_tier_for_dossier(MAX_DOSSIER_TIER), 3);
        assert_eq!(codex_tier_for_dossier(0), 0, "a failed read files nothing");

        // A clean reader hears the reassuring version of both lines.
        let (_, clean) = compose_dossier(&subject, Attitude::Friendly, MARGIN_CRIMES, 0, &[]);
        let note = clean.bearing.unwrap().crime_note.expect("crime note");
        assert!(note.contains("nothing ill"), "{note}");
    }

    /// Even a botched read still tells you who you're looking at — the
    /// identity block is tier 0, not a reward.
    #[test]
    fn dossier_identity_survives_a_failed_check() {
        let (summary, failed) = compose_failure(&test_subject());
        assert!(summary.contains("gives nothing away"));
        assert!(failed.failed);
        assert_eq!(failed.tier, 0);
        assert!(failed.bearing.is_none());
        assert_eq!(failed.name, "Guard");
        assert_eq!(failed.occupation.as_deref(), Some("Watch Sergeant"));
        assert_eq!(failed.description, "Broad-shouldered and bored.");
        // ...but nothing that had to be earned.
        assert!(failed.lore.is_none());
        assert!(failed.factions.is_empty());
    }

    #[test]
    fn cache_expires_after_cooldown() {
        let mut memory = SocialReadMemory::default();
        let player = PlayerId(1);
        memory.store(player, 100.0, "s".into(), NpcDossier::default());
        assert!(memory.fresh(player, 100.0).is_some());
        assert!(memory
            .fresh(player, 100.0 + SOCIAL_READ_COOLDOWN_SECS - 0.1)
            .is_some());
        assert!(memory
            .fresh(player, 100.0 + SOCIAL_READ_COOLDOWN_SECS)
            .is_none());
        assert!(memory.fresh(PlayerId(2), 100.0).is_none(), "per-player");
    }

    // ------------------------------------------------------------------
    // Command processing
    // ------------------------------------------------------------------

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::game::resources::PendingGameCommands>()
            .init_resource::<crate::game::resources::PendingGameUiEvents>()
            .init_resource::<PendingSocialReadLines>()
            .init_resource::<crate::codex::PendingCodexUpdates>()
            .init_resource::<crate::world::floor_map::FloorMaps>()
            .init_resource::<crate::world::object_registry::ObjectRegistry>()
            .insert_resource(FactionInterner::build(["emberbrook_watch"].into_iter()))
            .insert_resource(
                crate::world::object_definitions::OverworldObjectDefinitions::load_from_disk(),
            )
            .insert_resource(crate::magic::resources::SpellDefinitions::load_from_disk())
            .insert_resource(
                crate::world::floor_definitions::FloorTilesetDefinitions::load_from_disk(),
            );
        app.add_systems(
            Update,
            (process_social_read, emit_social_read_lines).chain(),
        );
        app
    }

    const TEST_SPACE: SpaceId = SpaceId(0);

    fn spawn_reader(app: &mut App, id: PlayerId, tile: (i32, i32)) -> Entity {
        app.world_mut()
            .spawn((
                crate::player::components::Player,
                PlayerIdentity::new(id),
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(tile.0, tile.1),
                BaseStats::default(),
                SkillSheet::default(),
                ChatLog::default(),
            ))
            .id()
    }

    fn spawn_npc(app: &mut App, object_id: u64, tile: (i32, i32)) -> Entity {
        spawn_npc_of(app, object_id, tile, "npc")
    }

    fn spawn_npc_of(
        app: &mut App,
        object_id: u64,
        tile: (i32, i32),
        definition_id: &str,
    ) -> Entity {
        app.world_mut()
            .spawn((
                Npc,
                crate::world::components::OverworldObject {
                    object_id,
                    definition_id: definition_id.to_owned(),
                    placement_seq: 0,
                },
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(tile.0, tile.1),
                SocialReadMemory::default(),
            ))
            .id()
    }

    fn queue(app: &mut App, player: PlayerId, command: GameCommand) {
        app.world_mut()
            .resource_mut::<crate::game::resources::PendingGameCommands>()
            .push_for_player(player, command);
    }

    fn reads_sent(app: &App, player: PlayerId) -> Vec<NpcDossier> {
        app.world()
            .resource::<crate::game::resources::PendingGameUiEvents>()
            .peer_events
            .get(&player)
            .into_iter()
            .flatten()
            .filter_map(|event| match event {
                crate::game::resources::GameUiEvent::OpenSocialRead { dossier, .. } => {
                    Some(dossier.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn chat_len(app: &App, entity: Entity) -> usize {
        app.world().get::<ChatLog>(entity).unwrap().lines.len()
    }

    #[test]
    fn details_rolls_once_then_replays_the_cache() {
        let mut app = test_app();
        let player = PlayerId(1);
        let reader = spawn_reader(&mut app, player, (5, 5));
        spawn_npc(&mut app, 77, (5, 6));

        queue(
            &mut app,
            player,
            GameCommand::RequestSocialRead { npc_object_id: 77 },
        );
        app.update();

        let sent = reads_sent(&app, player);
        assert_eq!(sent.len(), 1, "one window payload");
        assert!(
            sent[0].check_line.starts_with("(Persuasion"),
            "{:?}",
            sent[0]
        );
        // Identity is unconditional, so it's there whichever way the d20 fell.
        assert!(!sent[0].name.is_empty());
        let chat_after_roll = chat_len(&app, reader);
        assert!(
            chat_after_roll > ChatLog::default().lines.len(),
            "fresh roll narrates"
        );

        // Second request inside the cooldown: identical lines, no new roll,
        // no second narrator line.
        queue(
            &mut app,
            player,
            GameCommand::RequestSocialRead { npc_object_id: 77 },
        );
        app.update();
        let sent = reads_sent(&app, player);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0], sent[1], "cooldown replays the cached read");
        assert_eq!(
            chat_len(&app, reader),
            chat_after_roll,
            "cache hit is silent"
        );
    }

    /// A successful read files the durable half into the People codex; the
    /// cached replay that follows must not file it a second time.
    #[test]
    fn details_queues_a_people_codex_update_once() {
        let mut app = test_app();
        let player = PlayerId(1);
        spawn_reader(&mut app, player, (5, 5));
        spawn_npc_of(&mut app, 77, (5, 6), "town_guard");

        // The check can fail, and a failed read files nothing. Loop until we
        // land a success so the assertion isn't at the mercy of one d20.
        let mut queued = Vec::new();
        for attempt in 0..40u64 {
            app.world_mut()
                .resource_mut::<crate::codex::PendingCodexUpdates>()
                .items
                .clear();
            // A fresh NPC each pass — re-asking the same one would replay its
            // 60s cache instead of rolling again.
            let object_id = 100 + attempt;
            spawn_npc_of(&mut app, object_id, (5, 6), "town_guard");
            queue(
                &mut app,
                player,
                GameCommand::RequestSocialRead {
                    npc_object_id: object_id,
                },
            );
            app.update();
            queued = app
                .world()
                .resource::<crate::codex::PendingCodexUpdates>()
                .items
                .clone();
            if !queued.is_empty() {
                break;
            }
        }

        let (owner, update) = queued.first().expect("a successful read in 40 tries");
        assert_eq!(*owner, player);
        match update {
            crate::codex::CodexUpdate::NpcTier {
                definition_id,
                tier,
            } => {
                assert_eq!(definition_id, "town_guard");
                assert!((1..=3).contains(tier), "codex tier out of range: {tier}");
            }
            other => panic!("expected an NpcTier update, got {other:?}"),
        }
    }

    #[test]
    fn inspect_is_peeked_not_consumed() {
        let mut app = test_app();
        let player = PlayerId(1);
        let reader = spawn_reader(&mut app, player, (5, 5));
        let npc = spawn_npc(&mut app, 77, (5, 6));

        queue(
            &mut app,
            player,
            GameCommand::Inspect {
                target: InspectTarget::Object(77),
            },
        );
        app.update();

        // The command must survive for `process_game_commands` (absent in
        // this app, so it stays queued).
        let commands = &app
            .world()
            .resource::<crate::game::resources::PendingGameCommands>()
            .commands;
        assert_eq!(commands.len(), 1, "inspect left in the queue");
        // But the read happened: summary narrated, cache populated, no window.
        assert!(chat_len(&app, reader) > ChatLog::default().lines.len());
        assert_eq!(
            app.world()
                .get::<SocialReadMemory>(npc)
                .unwrap()
                .reads
                .len(),
            1
        );
        assert!(
            reads_sent(&app, player).is_empty(),
            "inspect opens no window"
        );
    }

    #[test]
    fn out_of_talk_range_reads_nothing() {
        let mut app = test_app();
        let player = PlayerId(1);
        let reader = spawn_reader(&mut app, player, (5, 5));
        let npc = spawn_npc(&mut app, 77, (5, 10));

        queue(
            &mut app,
            player,
            GameCommand::Inspect {
                target: InspectTarget::Object(77),
            },
        );
        queue(
            &mut app,
            player,
            GameCommand::RequestSocialRead { npc_object_id: 77 },
        );
        app.update();

        assert_eq!(chat_len(&app, reader), ChatLog::default().lines.len());
        assert!(app
            .world()
            .get::<SocialReadMemory>(npc)
            .unwrap()
            .reads
            .is_empty());
        assert!(reads_sent(&app, player).is_empty());
    }
}
