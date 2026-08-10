//! Guilt: the per-NPC, per-player grudge counter that makes hostility *earned*
//! rather than authored.
//!
//! The tag model in [`crate::npc::hostility`] is static — a wolf hunts
//! `livestock` forever and a guard's opinion of you never changes. Guilt is the
//! dynamic axis layered on top: hurt something, and every living member of its
//! social factions remembers *you specifically*.
//!
//! Three concepts, deliberately kept apart:
//!
//! - `Faction` (in `npc::components`) is the **combat side** —
//!   PlayerSide/MonsterSide/Neutral, symmetric enmity.
//! - `TagProfile.identity` is **what a creature is** — `beast`, `undead`.
//! - [`FactionMembership`] here is **who it answers to** — `emberbrook_watch`.
//!   Interned into its own 64-bit mask so it never competes with identity tags
//!   for the tag interner's budget.
//!
//! Guilt lives on the NPC ([`KnownGuilty`]), not on the player. That means a
//! guard spawned after your crime is genuinely unaware of it, and killing every
//! witness really does bury the evidence. The cost is that an offense writes to
//! every live faction member — batched through [`PendingGuiltEvents`] so it is
//! one pass per frame regardless of how many offenses landed.
//!
//! Effects are capped into tiers ([`GuiltTier`]) even though the number is not:
//! past `SHUNNED_THRESHOLD` an NPC refuses to talk or trade, past
//! `WANTED_THRESHOLD` it attacks on sight (if it has combat AI at all).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::npc::hostility::{TagInterner, TagMask};
use crate::player::components::{ChatLog, PlayerId, PlayerIdentity};

/// Interned social factions. Same bit-per-string representation as
/// [`TagMask`], but a separate interner (and therefore a separate 64-entry
/// budget) so allegiances and identity tags never crowd each other out.
pub type FactionMask = TagMask;

/// Guilt added for a single damaging blow against a faction member.
pub const ATTACK_GUILT: u32 = 10;

/// Guilt added for killing a faction member. Deliberately clear of
/// [`WANTED_THRESHOLD`] on its own: one murder makes you a wanted criminal
/// outright, with no accumulation needed.
pub const KILL_GUILT: u32 = 70;

/// Repeated hits on the *same* victim inside this window charge guilt only
/// once, so a damage-over-time tick or a flurry of fast swings doesn't ratchet
/// a player to Wanted for what is, narratively, one attack.
pub const ATTACK_DEBOUNCE_SECONDS: f32 = 3.0;

/// Guilt at or above this refuses conversation and trade.
pub const SHUNNED_THRESHOLD: u32 = 31;

/// Guilt at or above this is hostile on sight.
pub const WANTED_THRESHOLD: u32 = 61;

/// What a given guilt total means. Ordered, so callers can write
/// `tier >= GuiltTier::Shunned`.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum GuiltTier {
    /// 0..SHUNNED_THRESHOLD — nothing happens.
    #[default]
    Clean,
    /// SHUNNED_THRESHOLD..WANTED_THRESHOLD — refuses to interact.
    Shunned,
    /// WANTED_THRESHOLD.. — attacks on sight.
    Wanted,
}

impl GuiltTier {
    pub fn of(points: u32) -> Self {
        if points >= WANTED_THRESHOLD {
            GuiltTier::Wanted
        } else if points >= SHUNNED_THRESHOLD {
            GuiltTier::Shunned
        } else {
            GuiltTier::Clean
        }
    }
}

/// String → bit interner for social factions. Wraps [`TagInterner`] (whose
/// build/resolve are exactly right) and adds the reverse lookup the chat lines
/// need to name a faction.
#[derive(Resource, Default)]
pub struct FactionInterner {
    inner: TagInterner,
}

impl FactionInterner {
    pub fn build<'a>(all_factions: impl Iterator<Item = &'a str>) -> Self {
        Self {
            inner: TagInterner::build(all_factions),
        }
    }

    pub fn resolve(&self, factions: &[String]) -> FactionMask {
        self.inner.resolve(factions)
    }

    /// Human-readable name for every faction in `mask`, for player-facing
    /// text. `emberbrook_watch` → `Emberbrook Watch`.
    pub fn display_names(&self, mask: FactionMask) -> Vec<String> {
        mask.bits()
            .filter_map(|bit| self.inner.name_for_bit(bit))
            .map(prettify_faction_id)
            .collect()
    }
}

/// `emberbrook_watch` → `Emberbrook Watch`. A `display_name:` YAML field could
/// override this later; title-casing the id covers every faction authored so
/// far.
fn prettify_faction_id(id: &str) -> String {
    id.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The social factions an NPC answers to, resolved from its definition's
/// `factions:` list. Derived at spawn by `resolve_npc_tag_components` and
/// **never persisted** — it is pure template data, like `TagProfile`.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FactionMembership {
    pub mask: FactionMask,
}

/// An NPC who will, for coin, wipe a player's guilt with the factions it
/// speaks for. Derived at spawn from the definition's `judge:` block, like
/// [`FactionMembership`] — the fee schedule is template data, never persisted.
#[derive(Component, Clone, Copy, Debug)]
pub struct Judge {
    /// Factions this Judge has the authority to absolve.
    pub clears: FactionMask,
    /// Fee in copper per point of guilt forgiven.
    pub copper_per_guilt_point: u32,
}

/// One player's standing with one NPC.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct GuiltEntry {
    pub player: PlayerId,
    pub points: u32,
}

/// Players this NPC personally holds a grudge against.
///
/// Stored as a `Vec` rather than a map on purpose: it is empty for almost every
/// NPC and never holds more than a handful of entries, and a `Vec` round-trips
/// through `serde_json` without the custom key codec a `HashMap<PlayerId, _>`
/// would need.
///
/// **Persisted** in `NpcStateDump` — unlike the template-derived components,
/// this is runtime state and would silently reset on every world reload
/// otherwise. Keying by `PlayerId` is safe across saves because it is the
/// character-id cast; object ids are reallocated on load and must never be used
/// as a key here.
#[derive(Component, Clone, Debug, Default, Deserialize, Serialize)]
pub struct KnownGuilty {
    #[serde(default)]
    pub entries: Vec<GuiltEntry>,
}

impl KnownGuilty {
    pub fn points(&self, player: PlayerId) -> u32 {
        self.entries
            .iter()
            .find(|e| e.player == player)
            .map(|e| e.points)
            .unwrap_or(0)
    }

    pub fn tier(&self, player: PlayerId) -> GuiltTier {
        GuiltTier::of(self.points(player))
    }

    pub fn add(&mut self, player: PlayerId, amount: u32) {
        match self.entries.iter_mut().find(|e| e.player == player) {
            Some(entry) => entry.points = entry.points.saturating_add(amount),
            None => self.entries.push(GuiltEntry {
                player,
                points: amount,
            }),
        }
    }

    pub fn clear_player(&mut self, player: PlayerId) {
        self.entries.retain(|e| e.player != player);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// What an NPC says when it turns a guilty player away.
pub const REFUSAL_LINE: &str = "I'll have no dealings with the likes of you.";

/// Whether this NPC's grudge is deep enough to refuse conversation and trade.
/// Shared by the talk and shop gates so the two can't drift apart.
///
/// Note this is a *lower* bar than the hostility gate in
/// `hostility::is_hostile_toward`: a Shunned player is snubbed, a Wanted one is
/// attacked (and, being attacked, is hardly in a position to shop).
pub fn refuses_interaction(guilt: Option<&KnownGuilty>, player: PlayerId) -> bool {
    guilt.is_some_and(|g| g.tier(player) >= GuiltTier::Shunned)
}

/// One request to change what a whole faction thinks of a player.
#[derive(Clone, Copy, Debug)]
pub enum GuiltEvent {
    /// The player wronged this faction: add `amount` to every live member.
    Offense {
        player: PlayerId,
        factions: FactionMask,
        amount: u32,
    },
    /// The debt is settled (killed by the faction, or a fine paid): zero the
    /// player's guilt on every live member.
    Clear {
        player: PlayerId,
        factions: FactionMask,
    },
}

/// Queue drained by [`apply_pending_guilt`]. Mirrors the `PendingNpcAggro`
/// pattern — combat has no Bevy events, only drained resource queues.
///
/// The attack debounce lives here rather than in its own resource so callers
/// can't forget it, and so the damage system (already at Bevy's system-param
/// ceiling) needs only one parameter for guilt.
#[derive(Resource, Default)]
pub struct PendingGuiltEvents {
    pub items: Vec<GuiltEvent>,
    debounce: GuiltDebounce,
}

impl PendingGuiltEvents {
    pub fn push(&mut self, event: GuiltEvent) {
        self.items.push(event);
    }

    /// Queue an attack offense, unless this attacker already charged for this
    /// victim inside [`ATTACK_DEBOUNCE_SECONDS`]. Returns whether it queued.
    pub fn push_attack(
        &mut self,
        player: PlayerId,
        factions: FactionMask,
        victim: Entity,
        now: f32,
    ) -> bool {
        if !self.debounce.should_charge(player, victim, now) {
            return false;
        }
        self.push(GuiltEvent::Offense {
            player,
            factions,
            amount: ATTACK_GUILT,
        });
        true
    }
}

/// Per-(attacker, victim) timestamps backing [`ATTACK_DEBOUNCE_SECONDS`].
#[derive(Default)]
struct GuiltDebounce {
    seen: HashMap<(PlayerId, Entity), f32>,
}

impl GuiltDebounce {
    /// Returns true if this hit should charge guilt, stamping the window when
    /// it does.
    fn should_charge(&mut self, player: PlayerId, victim: Entity, now: f32) -> bool {
        let key = (player, victim);
        match self.seen.get(&key) {
            Some(last) if now - *last < ATTACK_DEBOUNCE_SECONDS => false,
            _ => {
                self.seen.insert(key, now);
                true
            }
        }
    }

    /// Drop windows that have expired. Called on drain so the map tracks live
    /// fights rather than growing for the process lifetime.
    fn prune(&mut self, now: f32) {
        self.seen
            .retain(|_, last| now - *last < ATTACK_DEBOUNCE_SECONDS);
    }
}

/// Drains [`PendingGuiltEvents`] in a single pass over every faction-bearing
/// NPC, applying each event to the members whose allegiance it names.
///
/// Tier crossings are narrated once per event, not once per NPC: the sweep
/// tracks the highest tier any affected member held before and after, so
/// wronging a twelve-guard watch produces one line, not twelve.
pub fn apply_pending_guilt(
    time: Res<Time>,
    mut pending: ResMut<PendingGuiltEvents>,
    interner: Res<FactionInterner>,
    mut members: Query<(Entity, &FactionMembership, Option<&mut KnownGuilty>)>,
    mut players: Query<(&PlayerIdentity, &mut ChatLog)>,
    mut commands: Commands,
) {
    pending.debounce.prune(time.elapsed_secs());
    if pending.items.is_empty() {
        return;
    }

    for event in std::mem::take(&mut pending.items) {
        let (player, factions) = match event {
            GuiltEvent::Offense {
                player, factions, ..
            }
            | GuiltEvent::Clear { player, factions } => (player, factions),
        };
        if factions.is_empty() {
            continue;
        }

        let mut before = GuiltTier::Clean;
        let mut after = GuiltTier::Clean;

        for (entity, membership, guilt) in &mut members {
            if !membership.mask.intersects(factions) {
                continue;
            }
            match event {
                GuiltEvent::Offense { amount, .. } => match guilt {
                    Some(mut guilt) => {
                        before = before.max(guilt.tier(player));
                        guilt.add(player, amount);
                        after = after.max(guilt.tier(player));
                    }
                    None => {
                        // First offense against this member: attach the
                        // component rather than pre-seeding every NPC with an
                        // empty one.
                        let mut fresh = KnownGuilty::default();
                        fresh.add(player, amount);
                        after = after.max(fresh.tier(player));
                        commands.entity(entity).insert(fresh);
                    }
                },
                GuiltEvent::Clear { .. } => {
                    if let Some(mut guilt) = guilt {
                        before = before.max(guilt.tier(player));
                        guilt.clear_player(player);
                    }
                }
            }
        }

        if before == after {
            continue;
        }
        let Some(mut chat) = players
            .iter_mut()
            .find(|(identity, _)| identity.id == player)
            .map(|(_, chat)| chat)
        else {
            continue;
        };
        let names = interner.display_names(factions);
        let Some(name) = names.first() else {
            continue;
        };
        let line = match after {
            GuiltTier::Clean => format!("[The {name} considers your debt settled.]"),
            GuiltTier::Shunned => format!("[The {name} eyes you coldly.]"),
            GuiltTier::Wanted => format!("[The {name} wants you dead!]"),
        };
        chat.push_line(line);
    }
}

/// Highest guilt any live member of `factions` holds against `player`. This is
/// the figure a Judge prices a fine from: with per-NPC ledgers there is no
/// single "faction opinion", so the worst outstanding grudge is what has to be
/// bought off.
fn worst_guilt_toward(
    members: &Query<(&FactionMembership, &KnownGuilty)>,
    factions: FactionMask,
    player: PlayerId,
) -> u32 {
    members
        .iter()
        .filter(|(membership, _)| membership.mask.intersects(factions))
        .map(|(_, guilt)| guilt.points(player))
        .max()
        .unwrap_or(0)
}

type JudgePlayerQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PlayerIdentity,
        &'static crate::world::components::SpaceResident,
        &'static crate::world::components::TilePosition,
        &'static mut crate::player::components::Inventory,
        &'static mut ChatLog,
    ),
    With<crate::player::components::Player>,
>;

/// Handles `GameCommand::PayGuiltFine`: price the player's outstanding guilt
/// with the Judge's factions, take the coin, and wipe the slate.
///
/// Drained out of `PendingGameCommands` in the `CommandIntercept` set, the same
/// way dialog and trade claim their own commands, rather than growing the
/// already-enormous `process_game_commands` match.
pub fn process_pay_guilt_fine(
    mut pending_commands: ResMut<crate::game::resources::PendingGameCommands>,
    mut players: JudgePlayerQuery,
    judges: Query<(
        &crate::world::components::OverworldObject,
        &crate::world::components::SpaceResident,
        &crate::world::components::TilePosition,
        &Judge,
    )>,
    members: Query<(&FactionMembership, &KnownGuilty)>,
    definitions: Res<crate::world::object_definitions::OverworldObjectDefinitions>,
    floors: crate::world::column::FloorGeometryParam,
    interner: Res<FactionInterner>,
    mut pending: ResMut<PendingGuiltEvents>,
) {
    // Cheap read-only probe first. `drain_matching` rebuilds the whole queue
    // and marks the resource changed, which is wasteful to do every frame for a
    // command a player issues once in a blue moon — read through `Deref` and
    // only take the queue when there is actually something of ours in it.
    let has_fine = pending_commands.commands.iter().any(|queued| {
        matches!(
            queued.command,
            crate::game::commands::GameCommand::PayGuiltFine { .. }
        )
    });
    if !has_fine {
        return;
    }
    let claimed = pending_commands.drain_matching(|command| match command {
        crate::game::commands::GameCommand::PayGuiltFine { npc_object_id } => Ok(npc_object_id),
        other => Err(other),
    });
    if claimed.is_empty() {
        return;
    }
    let geometry = floors.geometry();

    for (queued_player_id, npc_object_id) in claimed {
        let Some(acting_player_id) = queued_player_id else {
            continue;
        };
        let Some((_, judge_resident, judge_tile, judge)) = judges
            .iter()
            .find(|(object, _, _, _)| object.object_id == npc_object_id)
        else {
            crate::game::helpers::refuse(acting_player_id, "PayGuiltFine", "not a judge");
            continue;
        };
        let Some((_, resident, tile, mut inventory, mut chat)) = players
            .iter_mut()
            .find(|(identity, ..)| identity.id == acting_player_id)
        else {
            continue;
        };
        // Same reach rule as Talk — the client only offers the verb in range.
        if resident.space_id != judge_resident.space_id
            || !geometry.talk_reachable(tile, judge_tile, judge_resident.space_id)
        {
            crate::game::helpers::refuse(acting_player_id, "PayGuiltFine", "judge out of range");
            continue;
        }

        let owed_points = worst_guilt_toward(&members, judge.clears, acting_player_id);
        if owed_points == 0 {
            chat.push_narrator("The judge waves you off. \"You owe nothing here.\"");
            continue;
        }
        let fee = owed_points.saturating_mul(judge.copper_per_guilt_point);
        if !crate::game::currency::spend_copper(&mut inventory, fee, &definitions) {
            chat.push_narrator(format!(
                "The judge shakes their head. \"The fine is {} — come back when you can pay it.\"",
                crate::game::currency::format_compact(fee)
            ));
            continue;
        }

        let names = interner.display_names(judge.clears);
        let absolved = names.first().cloned().unwrap_or_else(|| "court".to_owned());
        chat.push_narrator(format!(
            "You pay {} and are pardoned by the {absolved}.",
            crate::game::currency::format_compact(fee)
        ));
        pending.push(GuiltEvent::Clear {
            player: acting_player_id,
            factions: judge.clears,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(bits: &[u8]) -> FactionMask {
        TagMask(bits.iter().fold(0u64, |acc, b| acc | (1 << b)))
    }

    const WATCH: u8 = 1;
    const TRIBE: u8 = 2;

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<PendingGuiltEvents>();
        app.insert_resource(FactionInterner::build(
            ["emberbrook_watch", "goblin_tribe"].into_iter(),
        ));
        app.add_systems(Update, apply_pending_guilt);
        app
    }

    fn spawn_member(app: &mut App, factions: FactionMask) -> Entity {
        app.world_mut()
            .spawn(FactionMembership { mask: factions })
            .id()
    }

    fn spawn_player(app: &mut App, id: PlayerId) -> Entity {
        app.world_mut()
            .spawn((PlayerIdentity::new(id), ChatLog::default()))
            .id()
    }

    fn guilt_of(app: &App, entity: Entity, player: PlayerId) -> u32 {
        app.world()
            .get::<KnownGuilty>(entity)
            .map(|g| g.points(player))
            .unwrap_or(0)
    }

    #[test]
    fn tier_boundaries_are_exact() {
        assert_eq!(GuiltTier::of(0), GuiltTier::Clean);
        assert_eq!(GuiltTier::of(30), GuiltTier::Clean);
        assert_eq!(GuiltTier::of(31), GuiltTier::Shunned);
        assert_eq!(GuiltTier::of(60), GuiltTier::Shunned);
        assert_eq!(GuiltTier::of(61), GuiltTier::Wanted);
        assert_eq!(GuiltTier::of(u32::MAX), GuiltTier::Wanted);
        // Ordered, so `>= Shunned` reads correctly at call sites.
        assert!(GuiltTier::Wanted > GuiltTier::Shunned);
        assert!(GuiltTier::Shunned > GuiltTier::Clean);
    }

    #[test]
    fn offense_propagates_to_every_member_and_skips_outsiders() {
        let mut app = test_app();
        let player = PlayerId(7);
        spawn_player(&mut app, player);
        let guard_a = spawn_member(&mut app, mask(&[WATCH]));
        let guard_b = spawn_member(&mut app, mask(&[WATCH]));
        let goblin = spawn_member(&mut app, mask(&[TRIBE]));

        app.world_mut()
            .resource_mut::<PendingGuiltEvents>()
            .push(GuiltEvent::Offense {
                player,
                factions: mask(&[WATCH]),
                amount: KILL_GUILT,
            });
        app.update();

        assert_eq!(guilt_of(&app, guard_a, player), KILL_GUILT);
        assert_eq!(guilt_of(&app, guard_b, player), KILL_GUILT);
        assert_eq!(
            guilt_of(&app, goblin, player),
            0,
            "an unrelated faction must not learn of the crime"
        );
    }

    #[test]
    fn a_single_kill_reaches_wanted() {
        let mut app = test_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        app.world_mut()
            .resource_mut::<PendingGuiltEvents>()
            .push(GuiltEvent::Offense {
                player,
                factions: mask(&[WATCH]),
                amount: KILL_GUILT,
            });
        app.update();

        assert_eq!(
            app.world().get::<KnownGuilty>(guard).unwrap().tier(player),
            GuiltTier::Wanted
        );
    }

    #[test]
    fn a_single_attack_is_not_enough_to_be_shunned() {
        let mut app = test_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        app.world_mut()
            .resource_mut::<PendingGuiltEvents>()
            .push(GuiltEvent::Offense {
                player,
                factions: mask(&[WATCH]),
                amount: ATTACK_GUILT,
            });
        app.update();

        assert_eq!(
            app.world().get::<KnownGuilty>(guard).unwrap().tier(player),
            GuiltTier::Clean
        );
    }

    #[test]
    fn clear_zeroes_only_the_named_faction() {
        let mut app = test_app();
        let player = PlayerId(3);
        spawn_player(&mut app, player);
        let guard = spawn_member(&mut app, mask(&[WATCH]));
        let goblin = spawn_member(&mut app, mask(&[TRIBE]));

        {
            let mut queue = app.world_mut().resource_mut::<PendingGuiltEvents>();
            queue.push(GuiltEvent::Offense {
                player,
                factions: mask(&[WATCH]),
                amount: KILL_GUILT,
            });
            queue.push(GuiltEvent::Offense {
                player,
                factions: mask(&[TRIBE]),
                amount: KILL_GUILT,
            });
        }
        app.update();
        assert_eq!(guilt_of(&app, guard, player), KILL_GUILT);
        assert_eq!(guilt_of(&app, goblin, player), KILL_GUILT);

        app.world_mut()
            .resource_mut::<PendingGuiltEvents>()
            .push(GuiltEvent::Clear {
                player,
                factions: mask(&[WATCH]),
            });
        app.update();

        assert_eq!(guilt_of(&app, guard, player), 0);
        assert_eq!(
            guilt_of(&app, goblin, player),
            KILL_GUILT,
            "settling with one faction must not absolve you elsewhere"
        );
    }

    #[test]
    fn guilt_is_per_player() {
        let mut app = test_app();
        let culprit = PlayerId(1);
        let innocent = PlayerId(2);
        spawn_player(&mut app, culprit);
        spawn_player(&mut app, innocent);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        app.world_mut()
            .resource_mut::<PendingGuiltEvents>()
            .push(GuiltEvent::Offense {
                player: culprit,
                factions: mask(&[WATCH]),
                amount: KILL_GUILT,
            });
        app.update();

        assert_eq!(guilt_of(&app, guard, culprit), KILL_GUILT);
        assert_eq!(guilt_of(&app, guard, innocent), 0);
    }

    #[test]
    fn a_member_spawned_after_the_crime_is_unaware() {
        let mut app = test_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let veteran = spawn_member(&mut app, mask(&[WATCH]));

        app.world_mut()
            .resource_mut::<PendingGuiltEvents>()
            .push(GuiltEvent::Offense {
                player,
                factions: mask(&[WATCH]),
                amount: KILL_GUILT,
            });
        app.update();

        let recruit = spawn_member(&mut app, mask(&[WATCH]));
        app.update();

        assert_eq!(guilt_of(&app, veteran, player), KILL_GUILT);
        assert_eq!(
            guilt_of(&app, recruit, player),
            0,
            "guilt is per-NPC: a guard who wasn't there knows nothing"
        );
    }

    #[test]
    fn debounce_suppresses_repeat_hits_inside_the_window() {
        let mut queue = PendingGuiltEvents::default();
        let player = PlayerId(1);
        let victim = Entity::from_raw_u32(1).unwrap();
        let watch = mask(&[WATCH]);

        assert!(queue.push_attack(player, watch, victim, 0.0));
        assert!(
            !queue.push_attack(player, watch, victim, 1.0),
            "a second blow inside the window is the same attack"
        );
        assert!(
            queue.push_attack(player, watch, victim, ATTACK_DEBOUNCE_SECONDS + 0.01),
            "once the window lapses it is a fresh offense"
        );
        assert_eq!(
            queue.items.len(),
            2,
            "only the charged hits should have been queued"
        );
    }

    #[test]
    fn debounce_is_per_victim_and_per_player() {
        let mut queue = PendingGuiltEvents::default();
        let a = PlayerId(1);
        let b = PlayerId(2);
        let victim_one = Entity::from_raw_u32(1).unwrap();
        let victim_two = Entity::from_raw_u32(2).unwrap();
        let watch = mask(&[WATCH]);

        assert!(queue.push_attack(a, watch, victim_one, 0.0));
        assert!(
            queue.push_attack(a, watch, victim_two, 0.0),
            "hitting a different NPC is a different offense"
        );
        assert!(
            queue.push_attack(b, watch, victim_one, 0.0),
            "another player's blow is charged to them independently"
        );
    }

    #[test]
    fn four_attacks_reach_shunned_but_not_wanted() {
        let mut app = test_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        // Four separate offenses, each past the debounce window.
        for _ in 0..4 {
            app.world_mut()
                .resource_mut::<PendingGuiltEvents>()
                .push(GuiltEvent::Offense {
                    player,
                    factions: mask(&[WATCH]),
                    amount: ATTACK_GUILT,
                });
            app.update();
        }

        assert_eq!(guilt_of(&app, guard, player), ATTACK_GUILT * 4);
        assert_eq!(
            app.world().get::<KnownGuilty>(guard).unwrap().tier(player),
            GuiltTier::Shunned,
            "a brawl gets you shunned; it takes a killing to be hunted"
        );
    }

    #[test]
    fn known_guilty_round_trips_through_json() {
        // A `Vec` rather than a `HashMap<PlayerId, _>` precisely so serde_json
        // needs no custom key codec — guard that.
        let mut ledger = KnownGuilty::default();
        ledger.add(PlayerId(42), KILL_GUILT);
        ledger.add(PlayerId(7), ATTACK_GUILT);

        let json = serde_json::to_string(&ledger).unwrap();
        let restored: KnownGuilty = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.points(PlayerId(42)), KILL_GUILT);
        assert_eq!(restored.points(PlayerId(7)), ATTACK_GUILT);
        assert_eq!(restored.points(PlayerId(99)), 0);

        // Rows written before the field existed must still load.
        let legacy: KnownGuilty = serde_json::from_str("{}").unwrap();
        assert!(legacy.is_empty());
    }

    #[test]
    fn repeated_offenses_accumulate_uncapped() {
        // The number is deliberately uncapped even though the *effects* cap at
        // Wanted — a serial killer's debt keeps growing, and so does their fine.
        let mut ledger = KnownGuilty::default();
        for _ in 0..10 {
            ledger.add(PlayerId(1), KILL_GUILT);
        }
        assert_eq!(ledger.points(PlayerId(1)), KILL_GUILT * 10);
        assert_eq!(ledger.tier(PlayerId(1)), GuiltTier::Wanted);
    }

    /// App wired for the Judge command path: the fine handler plus the
    /// propagation sweep it queues its `Clear` into.
    fn judge_app() -> App {
        let mut app = test_app();
        app.init_resource::<crate::game::resources::PendingGameCommands>()
            .init_resource::<crate::world::floor_map::FloorMaps>()
            .insert_resource(
                crate::world::object_definitions::OverworldObjectDefinitions::load_from_disk(),
            )
            .insert_resource(
                crate::world::floor_definitions::FloorTilesetDefinitions::load_from_disk(),
            );
        // Fine handler first, so the `Clear` it queues is swept the same frame.
        app.add_systems(Update, process_pay_guilt_fine.before(apply_pending_guilt));
        app
    }

    const TEST_SPACE: crate::world::components::SpaceId = crate::world::components::SpaceId(0);

    fn spawn_judge(app: &mut App, object_id: u64, tile: (i32, i32), per_point: u32) -> Entity {
        app.world_mut()
            .spawn((
                crate::world::components::OverworldObject {
                    object_id,
                    definition_id: "judge".to_owned(),
                    placement_seq: 0,
                },
                crate::world::components::SpaceResident {
                    space_id: TEST_SPACE,
                },
                crate::world::components::TilePosition::ground(tile.0, tile.1),
                Judge {
                    clears: mask(&[WATCH]),
                    copper_per_guilt_point: per_point,
                },
            ))
            .id()
    }

    fn spawn_paying_player(app: &mut App, id: PlayerId, tile: (i32, i32), gold: u32) -> Entity {
        let mut inventory = crate::player::components::Inventory::default();
        inventory.backpack_slots[0] = Some(crate::player::components::InventoryStack::item(
            crate::game::currency::GOLD_TYPE_ID,
            crate::world::map_layout::ObjectProperties::new(),
            gold,
        ));
        app.world_mut()
            .spawn((
                crate::player::components::Player,
                PlayerIdentity::new(id),
                crate::world::components::SpaceResident {
                    space_id: TEST_SPACE,
                },
                crate::world::components::TilePosition::ground(tile.0, tile.1),
                inventory,
                ChatLog::default(),
            ))
            .id()
    }

    fn send_pay_fine(app: &mut App, player: PlayerId, npc_object_id: u64) {
        app.world_mut()
            .resource_mut::<crate::game::resources::PendingGameCommands>()
            .push_for_player(
                player,
                crate::game::commands::GameCommand::PayGuiltFine { npc_object_id },
            );
    }

    #[test]
    fn paying_a_judge_clears_guilt_and_takes_the_coin() {
        let mut app = judge_app();
        let player = PlayerId(1);
        let player_entity = spawn_paying_player(&mut app, player, (5, 5), 3);
        spawn_judge(&mut app, 77, (5, 6), 4);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        // Commit a murder, then settle up.
        app.world_mut()
            .resource_mut::<PendingGuiltEvents>()
            .push(GuiltEvent::Offense {
                player,
                factions: mask(&[WATCH]),
                amount: KILL_GUILT,
            });
        app.update();
        assert_eq!(guilt_of(&app, guard, player), KILL_GUILT);

        let purse_before = crate::game::currency::purse_total_copper(
            app.world()
                .get::<crate::player::components::Inventory>(player_entity)
                .unwrap(),
        );
        send_pay_fine(&mut app, player, 77);
        app.update();

        assert_eq!(
            guilt_of(&app, guard, player),
            0,
            "the fine must clear the guilt it was priced from"
        );
        let purse_after = crate::game::currency::purse_total_copper(
            app.world()
                .get::<crate::player::components::Inventory>(player_entity)
                .unwrap(),
        );
        assert_eq!(
            purse_before - purse_after,
            KILL_GUILT * 4,
            "fee is copper_per_guilt_point x the worst outstanding grudge"
        );
    }

    #[test]
    fn a_player_who_cannot_pay_keeps_both_coin_and_guilt() {
        let mut app = judge_app();
        let player = PlayerId(1);
        // One gold (240c) against a 70x12 = 840c fine.
        let player_entity = spawn_paying_player(&mut app, player, (5, 5), 1);
        spawn_judge(&mut app, 77, (5, 6), 12);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        app.world_mut()
            .resource_mut::<PendingGuiltEvents>()
            .push(GuiltEvent::Offense {
                player,
                factions: mask(&[WATCH]),
                amount: KILL_GUILT,
            });
        app.update();

        send_pay_fine(&mut app, player, 77);
        app.update();

        assert_eq!(
            guilt_of(&app, guard, player),
            KILL_GUILT,
            "an unaffordable fine must not clear guilt"
        );
        assert_eq!(
            crate::game::currency::purse_total_copper(
                app.world()
                    .get::<crate::player::components::Inventory>(player_entity)
                    .unwrap(),
            ),
            crate::game::currency::COPPER_PER_GOLD,
            "and must not take any coin"
        );
    }

    #[test]
    fn an_out_of_range_judge_is_refused() {
        let mut app = judge_app();
        let player = PlayerId(1);
        let player_entity = spawn_paying_player(&mut app, player, (5, 5), 10);
        // Well beyond TALK_RANGE_TILES.
        spawn_judge(&mut app, 77, (40, 40), 4);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        app.world_mut()
            .resource_mut::<PendingGuiltEvents>()
            .push(GuiltEvent::Offense {
                player,
                factions: mask(&[WATCH]),
                amount: KILL_GUILT,
            });
        app.update();

        send_pay_fine(&mut app, player, 77);
        app.update();

        assert_eq!(guilt_of(&app, guard, player), KILL_GUILT);
        assert_eq!(
            crate::game::currency::purse_total_copper(
                app.world()
                    .get::<crate::player::components::Inventory>(player_entity)
                    .unwrap(),
            ),
            10 * crate::game::currency::COPPER_PER_GOLD,
            "shouting at a distant magistrate must not cost anything"
        );
    }

    #[test]
    fn the_fine_handler_leaves_other_commands_in_the_queue() {
        // `process_game_commands` drains whatever CommandIntercept leaves, so a
        // claimer that eats unrelated commands would silently break movement.
        let mut app = judge_app();
        let player = PlayerId(1);
        spawn_paying_player(&mut app, player, (5, 5), 10);
        spawn_judge(&mut app, 77, (5, 6), 4);

        {
            let mut queue = app
                .world_mut()
                .resource_mut::<crate::game::resources::PendingGameCommands>();
            queue.push_for_player(
                player,
                crate::game::commands::GameCommand::MovePlayer {
                    delta: crate::game::commands::MoveDelta { x: 1, y: 0 },
                    climb: false,
                },
            );
            queue.push_for_player(
                player,
                crate::game::commands::GameCommand::PayGuiltFine { npc_object_id: 77 },
            );
        }
        app.update();

        let queue = app
            .world()
            .resource::<crate::game::resources::PendingGameCommands>();
        assert_eq!(queue.commands.len(), 1, "only the fine should be claimed");
        assert!(
            matches!(
                queue.commands[0].command,
                crate::game::commands::GameCommand::MovePlayer { .. }
            ),
            "the untouched MovePlayer must survive for the main dispatcher"
        );
        assert_eq!(queue.commands[0].player_id, Some(player));
    }

    #[test]
    fn faction_ids_prettify_for_player_facing_text() {
        assert_eq!(prettify_faction_id("emberbrook_watch"), "Emberbrook Watch");
        assert_eq!(prettify_faction_id("watch"), "Watch");
        assert_eq!(prettify_faction_id(""), "");
    }

    #[test]
    fn interner_round_trips_names_through_the_mask() {
        let interner = FactionInterner::build(["emberbrook_watch", "goblin_tribe"].into_iter());
        let watch = interner.resolve(&["emberbrook_watch".to_owned()]);
        assert!(!watch.is_empty());
        assert_eq!(interner.display_names(watch), vec!["Emberbrook Watch"]);
        // Unknown factions resolve to nothing rather than panicking, matching
        // the tag interner.
        assert!(interner.resolve(&["nonexistent".to_owned()]).is_empty());
    }
}
