//! Tag-based hostility: the data model behind `tags:` / `hostile_towards:` /
//! `flees_from:` in object YAML.
//!
//! Tags are free-form strings in YAML, interned once at startup into bits of
//! a `u64` so the per-tick detection filters stay a couple of integer ops.
//! The unified predicate is [`is_hostile_toward`]: faction enmity (the
//! pre-existing PlayerSide↔MonsterSide axis) OR tag overlap between the
//! aggressor's `hostile_towards` and the target's identity. Tag hostility is
//! asymmetric by design — a wolf is hostile toward `livestock`, the sheep
//! bears the tag without reciprocating.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::combat::components::CombatLeash;
use crate::npc::components::{Companion, Faction, HostileBehavior, Npc, PreyBehavior};
use crate::npc::guilt::{CrimeMemory, FactionInterner, FactionMembership, GuiltTier, Judge};
use crate::player::components::PlayerId;
use crate::world::components::OverworldObject;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// A set of interned tags as a bitmask. Cheap to copy/compare; capacity 64
/// distinct tag strings game-wide (bit 0 reserved for the implicit `player`
/// identity).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TagMask(pub u64);

impl TagMask {
    pub const EMPTY: TagMask = TagMask(0);
    /// The implicit identity every player carries. Reserved at interner
    /// build time so `hostile_towards: [player]` works in YAML.
    pub const PLAYER: TagMask = TagMask(1);

    pub fn intersects(self, other: TagMask) -> bool {
        self.0 & other.0 != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The bit indices set in this mask, ascending. Used to turn a mask back
    /// into the strings it was interned from (see
    /// `guilt::FactionInterner::display_names`).
    pub fn bits(self) -> impl Iterator<Item = u8> {
        (0u8..64).filter(move |bit| self.0 & (1 << bit) != 0)
    }
}

/// The reserved spelling of the implicit player tag.
pub const PLAYER_TAG: &str = "player";

/// String → bit interner, built once from every loaded object definition's
/// `tags` / `hostile_towards` / `flees_from` lists. A resource so spawn-time
/// resolution and (future) script spawns share one mapping.
#[derive(Resource, Default)]
pub struct TagInterner {
    bits: HashMap<String, u8>,
    /// Tags beyond the 64-bit capacity, warned once at build time and
    /// resolved to the empty mask thereafter.
    overflowed: Vec<String>,
}

impl TagInterner {
    /// Build from an iterator over every tag string mentioned anywhere.
    pub fn build<'a>(all_tags: impl Iterator<Item = &'a str>) -> Self {
        let mut interner = TagInterner::default();
        interner.bits.insert(PLAYER_TAG.to_owned(), 0);
        for tag in all_tags {
            if interner.bits.contains_key(tag) {
                continue;
            }
            let next = interner.bits.len() as u8;
            if next >= 64 {
                warn!(
                    "tag interner is full (64 tags); ignoring tag '{tag}' — \
                     creatures using it will not match on it"
                );
                interner.overflowed.push(tag.to_owned());
                continue;
            }
            interner.bits.insert(tag.to_owned(), next);
        }
        interner
    }

    /// Reverse lookup: the string interned at `bit`. Linear over at most 64
    /// entries — only used for player-facing text, never per tick.
    pub fn name_for_bit(&self, bit: u8) -> Option<&str> {
        self.bits
            .iter()
            .find(|(_, interned)| **interned == bit)
            .map(|(name, _)| name.as_str())
    }

    /// Resolve a YAML tag list to a mask. Unknown tags resolve to nothing
    /// (they were either overflowed at build time or never declared).
    pub fn resolve(&self, tags: &[String]) -> TagMask {
        let mut mask = 0u64;
        for tag in tags {
            if let Some(bit) = self.bits.get(tag.as_str()) {
                mask |= 1 << bit;
            }
        }
        TagMask(mask)
    }
}

/// Per-NPC resolved tag data. Re-derived from the object definition at spawn
/// and on snapshot restore (like `Barks`) — never persisted.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct TagProfile {
    /// What this creature *is* (`tags:`).
    pub identity: TagMask,
    /// What it attacks on sight (`hostile_towards:`).
    pub hostile_towards: TagMask,
    /// What it runs from (`flees_from:`).
    pub flees_from: TagMask,
}

/// The side of the predicate doing the hating: what this creature fights on,
/// what it hunts, and who it personally holds a grudge against.
#[derive(Clone, Copy, Debug)]
pub struct Aggressor<'a> {
    pub faction: Faction,
    pub hostile_towards: TagMask,
    /// This NPC's own grudge ledger, if it has one. `None` for creatures that
    /// have never been wronged (the overwhelmingly common case) and for
    /// players.
    pub guilt: Option<&'a CrimeMemory>,
}

impl<'a> Aggressor<'a> {
    /// The common case at call sites that have the components in hand.
    pub fn new(faction: Faction, hostile_towards: TagMask, guilt: Option<&'a CrimeMemory>) -> Self {
        Self {
            faction,
            hostile_towards,
            guilt,
        }
    }
}

/// The side being judged: what it fights on, what it *is*, and — when it's a
/// player — who it is, so guilt can be looked up.
#[derive(Clone, Copy, Debug)]
pub struct Subject {
    pub faction: Faction,
    pub identity: TagMask,
    /// `Some` only for players. NPCs never accrue guilt against each other.
    pub player_id: Option<PlayerId>,
}

impl Subject {
    pub fn new(faction: Faction, identity: TagMask, player_id: Option<PlayerId>) -> Self {
        Self {
            faction,
            identity,
            player_id,
        }
    }
}

/// The one hostility predicate. Three independent gates, any of which suffices:
///
/// 1. **Faction enmity** — symmetric, the PlayerSide↔MonsterSide axis.
/// 2. **Tag hostility** — asymmetric by design: a wolf is hostile toward
///    `livestock`, the sheep bears the tag without reciprocating.
/// 3. **Guilt** — earned rather than authored: a player past
///    [`WANTED_THRESHOLD`] with this NPC is attacked on sight even by a guard
///    that is otherwise player-friendly.
///
/// Used by NPC target acquisition and by the per-viewer `is_hostile` projection
/// flag — which is why gate 3 makes a guard render red to the criminal who
/// robbed it and peaceful to everyone standing next to them.
pub fn is_hostile_toward(a: Aggressor<'_>, b: Subject) -> bool {
    if a.faction.is_enemy_of(b.faction) || a.hostile_towards.intersects(b.identity) {
        return true;
    }
    match (a.guilt, b.player_id) {
        (Some(guilt), Some(player)) => guilt.tier(player) >= GuiltTier::Wanted,
        _ => false,
    }
}

/// Detection numbers for a prey NPC whose definition has no `npc_behavior:`
/// block to borrow them from. Also the civilian crime-witness radius fallback
/// (`npc::systems::resolve_witnessed_crime`).
pub(crate) const DEFAULT_PREY_DETECT_TILES: i32 = 6;

/// Resolves the tag-model components for every freshly-added NPC, whatever
/// path spawned it (map placement, spawn group, summon, admin spawn, snapshot
/// restore — they all insert `Npc`, so `Added<Npc>` catches them all without
/// threading the interner through every spawn signature):
///
/// - `TagProfile` from the definition's `tags` / `hostile_towards` /
///   `flees_from` (only when any is non-empty),
/// - `PreyBehavior` when `flees_from` is non-empty,
/// - `HostileBehavior` + `CombatLeash` when `hostile_towards` is non-empty and
///   the spawn behavior didn't already attach them (a guard on a plain `roam`
///   still needs combat AI to fight monsters),
/// - `Faction`: an explicit definition `faction:` wins; otherwise tag-bearing
///   NPCs without a faction land on `Neutral` (targetable, nobody's faction
///   enemy). Companions are exempt from faction rewriting — their side is the
///   summoner's, set at spawn.
///
/// Runs one command-application after the spawn itself, which is well inside
/// the NPC's first AI step delay.
pub fn resolve_npc_tag_components(
    interner: Res<TagInterner>,
    faction_interner: Res<FactionInterner>,
    definitions: Res<OverworldObjectDefinitions>,
    fresh: Query<
        (
            Entity,
            &OverworldObject,
            Option<&Faction>,
            Has<HostileBehavior>,
            Has<Companion>,
        ),
        Added<Npc>,
    >,
    mut commands: Commands,
) {
    for (entity, object, faction, has_hostile, is_companion) in &fresh {
        let Some(def) = definitions.get(&object.definition_id) else {
            continue;
        };
        let profile = TagProfile {
            identity: interner.resolve(&def.tags),
            hostile_towards: interner.resolve(&def.hostile_towards),
            flees_from: interner.resolve(&def.flees_from),
        };
        let has_tag_data = !profile.identity.is_empty()
            || !profile.hostile_towards.is_empty()
            || !profile.flees_from.is_empty();
        if has_tag_data {
            commands.entity(entity).insert(profile);
        }

        // Social factions. Like `TagProfile` this is template data, so it is
        // re-derived here rather than persisted — but the `CrimeMemory` grudges
        // keyed against it *are* persisted, and are restored separately by the
        // snapshot loader.
        let factions = faction_interner.resolve(&def.factions);
        if !factions.is_empty() {
            commands
                .entity(entity)
                .insert(FactionMembership { mask: factions });
        }
        if let Some(judge) = def.judge.as_ref() {
            commands.entity(entity).insert(Judge {
                clears: faction_interner.resolve(&judge.clears_factions),
                copper_per_guilt_point: judge.copper_per_guilt_point,
            });
        }
        // Protector duty (`protects_factions:`): template data like the
        // membership above. The witness system requires combat AI to act on
        // it, but the component is attached regardless so data errors surface
        // as inert protectors rather than silently-dropped YAML.
        let protects = faction_interner.resolve(&def.protects_factions);
        if !protects.is_empty() {
            commands
                .entity(entity)
                .insert(crate::npc::witness::Protector { protects });
        }

        if !profile.flees_from.is_empty() {
            let (detect, los) = def
                .npc_behavior
                .as_ref()
                .map(|b| (b.detect_distance_tiles.max(1), b.requires_line_of_sight))
                .unwrap_or((DEFAULT_PREY_DETECT_TILES, true));
            commands.entity(entity).insert(PreyBehavior {
                detect_distance_tiles: detect,
                requires_line_of_sight: los,
            });
        }

        if !profile.hostile_towards.is_empty() && !has_hostile {
            if let Some(b) = def.npc_behavior.as_ref() {
                let detect = b.detect_distance_tiles.max(1);
                let disengage = b.disengage_distance_tiles.max(detect);
                commands.entity(entity).insert((
                    HostileBehavior {
                        detect_distance_tiles: detect,
                        disengage_distance_tiles: disengage,
                        alert_duration_seconds: b.alert_duration_seconds,
                        requires_line_of_sight: b.requires_line_of_sight,
                        perception: b.perception,
                    },
                    CombatLeash {
                        max_distance_tiles: disengage,
                    },
                ));
            }
        }

        if is_companion {
            continue;
        }
        if let Some(explicit) = def.faction {
            let explicit: Faction = explicit.into();
            if faction != Some(&explicit) {
                commands.entity(entity).insert(explicit);
            }
        } else if faction.is_none() && has_tag_data {
            commands.entity(entity).insert(Faction::Neutral);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interner_reserves_player_bit_and_interns_in_order() {
        let tags = ["beast", "predator", "beast", "livestock"];
        let interner = TagInterner::build(tags.into_iter());
        let player = interner.resolve(&[PLAYER_TAG.to_owned()]);
        assert_eq!(player, TagMask::PLAYER);
        let beast = interner.resolve(&["beast".to_owned()]);
        let predator = interner.resolve(&["predator".to_owned()]);
        assert_ne!(beast, TagMask::EMPTY);
        assert_ne!(predator, TagMask::EMPTY);
        assert!(!beast.intersects(predator));
        let both = interner.resolve(&["beast".to_owned(), "predator".to_owned()]);
        assert!(both.intersects(beast) && both.intersects(predator));
        // Unknown tag resolves to nothing rather than panicking.
        assert_eq!(interner.resolve(&["dragon".to_owned()]), TagMask::EMPTY);
    }

    #[test]
    fn interner_overflow_warns_and_ignores() {
        let many: Vec<String> = (0..80).map(|i| format!("tag{i}")).collect();
        let interner = TagInterner::build(many.iter().map(|s| s.as_str()));
        // First 63 custom tags fit (bit 0 is `player`); the rest resolve empty.
        assert_ne!(interner.resolve(&["tag0".to_owned()]), TagMask::EMPTY);
        assert_ne!(interner.resolve(&["tag62".to_owned()]), TagMask::EMPTY);
        assert_eq!(interner.resolve(&["tag63".to_owned()]), TagMask::EMPTY);
    }

    #[test]
    fn hostility_predicate_truth_table() {
        use Faction::*;
        let wolf_hostile = TagMask(0b10); // hostile_towards: livestock
        let sheep_identity = TagMask(0b10); // tags: [livestock]
        let none = TagMask::EMPTY;
        let creature = |faction, identity| Subject::new(faction, identity, None);

        // Faction axis: only PlayerSide↔MonsterSide are enemies.
        assert!(is_hostile_toward(
            Aggressor::new(PlayerSide, none, None),
            creature(MonsterSide, none)
        ));
        assert!(is_hostile_toward(
            Aggressor::new(MonsterSide, none, None),
            creature(PlayerSide, none)
        ));
        assert!(!is_hostile_toward(
            Aggressor::new(PlayerSide, none, None),
            creature(PlayerSide, none)
        ));
        assert!(!is_hostile_toward(
            Aggressor::new(Neutral, none, None),
            creature(PlayerSide, none)
        ));
        assert!(!is_hostile_toward(
            Aggressor::new(Neutral, none, None),
            creature(MonsterSide, none)
        ));
        assert!(!is_hostile_toward(
            Aggressor::new(MonsterSide, none, None),
            creature(Neutral, none)
        ));

        // Tag axis is asymmetric: wolf → sheep, never sheep → wolf.
        assert!(is_hostile_toward(
            Aggressor::new(Neutral, wolf_hostile, None),
            creature(Neutral, sheep_identity)
        ));
        assert!(!is_hostile_toward(
            Aggressor::new(Neutral, none, None),
            creature(Neutral, wolf_hostile)
        ));

        // Either gate suffices.
        assert!(is_hostile_toward(
            Aggressor::new(MonsterSide, none, None),
            creature(PlayerSide, sheep_identity)
        ));
        assert!(is_hostile_toward(
            Aggressor::new(PlayerSide, wolf_hostile, None),
            creature(Neutral, sheep_identity)
        ));
    }

    #[test]
    fn guilt_gate_makes_a_friendly_guard_hostile_to_the_criminal_only() {
        use crate::npc::guilt::CrimeKind;

        let culprit = PlayerId(1);
        let bystander = PlayerId(2);
        let ledger = CrimeMemory::test_knowing(culprit, &[CrimeKind::Kill]);

        // A town guard: PlayerSide, hostile only toward monster tags, so both
        // players are normally safe from it.
        let guard = |guilt| Aggressor::new(Faction::PlayerSide, TagMask::EMPTY, guilt);
        let player = |id| Subject::new(Faction::PlayerSide, TagMask::PLAYER, Some(id));

        assert!(
            !is_hostile_toward(guard(None), player(culprit)),
            "a guard with no ledger is hostile to nobody on the player side"
        );
        assert!(
            is_hostile_toward(guard(Some(&ledger)), player(culprit)),
            "past the Wanted threshold the guard turns on the criminal"
        );
        assert!(
            !is_hostile_toward(guard(Some(&ledger)), player(bystander)),
            "guilt is per-player: the bystander is still safe"
        );
    }

    #[test]
    fn guilt_below_wanted_is_not_hostile() {
        use crate::npc::guilt::CrimeKind;

        let culprit = PlayerId(1);
        // Shunned-but-not-Wanted: refuses to talk, but does not draw steel.
        let ledger = CrimeMemory::test_knowing(culprit, &[CrimeKind::Attack; 4]);
        assert_eq!(ledger.tier(culprit), GuiltTier::Shunned);

        assert!(!is_hostile_toward(
            Aggressor::new(Faction::PlayerSide, TagMask::EMPTY, Some(&ledger)),
            Subject::new(Faction::PlayerSide, TagMask::PLAYER, Some(culprit))
        ));
    }

    #[test]
    fn guilt_never_applies_between_npcs() {
        let ledger = CrimeMemory::test_knowing(PlayerId(1), &[crate::npc::guilt::CrimeKind::Kill]);
        // An NPC subject carries no player_id, so the guilt gate can't fire
        // even though the ledger is loaded.
        assert!(!is_hostile_toward(
            Aggressor::new(Faction::Neutral, TagMask::EMPTY, Some(&ledger)),
            Subject::new(Faction::Neutral, TagMask(0b10), None)
        ));
    }
}
