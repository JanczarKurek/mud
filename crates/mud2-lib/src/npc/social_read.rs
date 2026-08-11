//! Social reads: a Persuasion-gated glimpse of what an NPC thinks of you.
//!
//! Inspecting an NPC (`GameCommand::Inspect`) implicitly rolls a Persuasion
//! check against the NPC's demeanor and drops a one-line summary into chat;
//! the "Details" context-menu verb (`GameCommand::RequestSocialRead`) replies
//! with the full read as `GameUiEvent::OpenSocialRead`. How much the read
//! reveals scales with the check's margin: a bare success reads the NPC's
//! attitude, +5 adds whether it knows of your crimes (and how bad), +10 adds
//! which factions' grudges it carries.
//!
//! Each (player, NPC) pair rolls at most once per [`SOCIAL_READ_COOLDOWN_SECS`]
//! — inside the window both paths replay the cached read verbatim, so the
//! check can't be spam-rerolled. The cache is session-only state, like
//! `world::hidden::Hidden`'s check schedule: it deliberately does not survive
//! a world reload.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::game::commands::{GameCommand, InspectTarget};
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

/// One resolved read, replayed verbatim until it expires.
#[derive(Clone, Debug)]
pub struct CachedSocialRead {
    /// Absolute `Time::elapsed_secs_f64()` past which a new roll is allowed.
    pub expires_at: f64,
    /// The one-line chat summary.
    pub summary: String,
    /// The full window body, pre-formatted.
    pub lines: Vec<String>,
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

    pub fn store(&mut self, player: PlayerId, now: f64, summary: String, lines: Vec<String>) {
        self.reads.insert(
            player,
            CachedSocialRead {
                expires_at: now + SOCIAL_READ_COOLDOWN_SECS,
                summary,
                lines,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Attitude {
    Friendly,
    Neutral,
    Wary,
    Hostile,
}

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

/// The successful read's summary + window body, tiered by margin.
pub fn compose_read(
    attitude: Attitude,
    margin: i32,
    guilt_points: u32,
    faction_names: &[String],
    npc_name: &str,
) -> (String, Vec<String>) {
    let summary = attitude_phrase(attitude, npc_name);
    let mut lines = vec![summary.clone()];
    if margin >= MARGIN_CRIMES {
        if guilt_points == 0 {
            lines.push("Your name means nothing ill to them.".to_owned());
        } else {
            lines.push(match GuiltTier::of(guilt_points) {
                GuiltTier::Clean => {
                    "They know of a misdeed of yours, but think it minor.".to_owned()
                }
                GuiltTier::Shunned => {
                    "They know enough of your crimes to want nothing to do with you.".to_owned()
                }
                GuiltTier::Wanted => "They know your crimes — enough to want you dead.".to_owned(),
            });
        }
    }
    if margin >= MARGIN_FACTIONS {
        if faction_names.is_empty() {
            lines.push("They carry no faction's grudge against you.".to_owned());
        } else {
            lines.push(format!(
                "Word of your crimes against the {} has reached them.",
                faction_names.join(", ")
            ));
        }
    }
    (summary, lines)
}

/// The failed read: the NPC gives nothing away.
pub fn compose_failure(npc_name: &str) -> (String, Vec<String>) {
    (
        format!("{npc_name} gives nothing away."),
        vec![format!("You can't get a read on {npc_name}.")],
    )
}

/// Roll (or replay) `player`'s read of one NPC. Returns the summary, the
/// window body, and whether this call performed a fresh roll.
#[allow(clippy::too_many_arguments)]
fn ensure_read(
    memory: &mut SocialReadMemory,
    now: f64,
    player: PlayerId,
    sheet: &SkillSheet,
    attributes: &crate::player::components::AttributeSet,
    npc_name: &str,
    npc_faction: Faction,
    hostile: bool,
    guilt: Option<&CrimeMemory>,
    interner: &FactionInterner,
) -> (String, Vec<String>, bool) {
    if let Some(cached) = memory.fresh(player, now) {
        return (cached.summary.clone(), cached.lines.clone(), false);
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

    let (summary, mut lines) = if result.success {
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
        let faction_names = interner.display_names(grudge_mask);
        compose_read(
            attitude,
            result.total - dc.total(),
            guilt_points,
            &faction_names,
            npc_name,
        )
    } else {
        compose_failure(npc_name)
    };
    lines.push(format!(
        "(Persuasion {} vs DC {})",
        result.total,
        dc.explain()
    ));

    memory.store(player, now, summary.clone(), lines.clone());
    (summary, lines, true)
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

        let (summary, lines, fresh) = ensure_read(
            &mut memory,
            now,
            player_id,
            sheet,
            &base_stats.attributes,
            &npc_name,
            social_faction,
            hostile,
            guilt,
            &interner,
        );

        if is_explicit {
            ui_events.push(
                player_id,
                crate::game::resources::GameUiEvent::OpenSocialRead {
                    npc_object_id,
                    npc_name,
                    lines,
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

    #[test]
    fn compose_read_tiers_by_margin() {
        let factions = vec!["Emberbrook Watch".to_owned()];
        let (_, bare) = compose_read(Attitude::Wary, 0, 40, &factions, "Guard");
        assert_eq!(bare.len(), 1, "bare success: attitude only");

        let (_, crimes) = compose_read(Attitude::Wary, MARGIN_CRIMES, 40, &factions, "Guard");
        assert_eq!(crimes.len(), 2, "margin 5 adds crime knowledge");
        assert!(crimes[1].contains("nothing to do with you"), "{crimes:?}");

        let (_, full) = compose_read(Attitude::Wary, MARGIN_FACTIONS, 40, &factions, "Guard");
        assert_eq!(full.len(), 3, "margin 10 adds faction grudges");
        assert!(full[2].contains("Emberbrook Watch"), "{full:?}");

        let (_, clean) = compose_read(Attitude::Friendly, MARGIN_FACTIONS, 0, &[], "Ann");
        assert!(clean[1].contains("nothing ill"), "{clean:?}");
        assert!(clean[2].contains("no faction"), "{clean:?}");
    }

    #[test]
    fn cache_expires_after_cooldown() {
        let mut memory = SocialReadMemory::default();
        let player = PlayerId(1);
        memory.store(player, 100.0, "s".into(), vec!["l".into()]);
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
        app.world_mut()
            .spawn((
                Npc,
                crate::world::components::OverworldObject {
                    object_id,
                    definition_id: "npc".to_owned(),
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

    fn reads_sent(app: &App, player: PlayerId) -> Vec<Vec<String>> {
        app.world()
            .resource::<crate::game::resources::PendingGameUiEvents>()
            .peer_events
            .get(&player)
            .into_iter()
            .flatten()
            .filter_map(|event| match event {
                crate::game::resources::GameUiEvent::OpenSocialRead { lines, .. } => {
                    Some(lines.clone())
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
        assert!(sent[0].len() >= 2, "body + check line: {:?}", sent[0]);
        assert!(
            sent[0].last().unwrap().starts_with("(Persuasion"),
            "{:?}",
            sent[0]
        );
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
