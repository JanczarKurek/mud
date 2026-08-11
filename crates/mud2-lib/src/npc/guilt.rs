//! Guilt: the per-NPC memory of *specific crimes* that makes hostility earned
//! rather than authored.
//!
//! The tag model in [`crate::npc::hostility`] is static — a wolf hunts
//! `livestock` forever and a guard's opinion of you never changes. Guilt is the
//! dynamic axis layered on top, and it is **witness-gated**: hurting or killing
//! a faction member creates a [`CrimeRecord`], but the record only enters an
//! NPC's [`CrimeMemory`] if that NPC actually saw the assault (or *was* the
//! surviving victim). A murder with no living witness leaves no trace — the
//! perfect crime.
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
//! Knowledge flows through three channels, all landing in
//! [`PendingCrimeLearns`] and applied by [`apply_crime_memory_updates`]:
//!
//! 1. **The victim** — a surviving victim knows first-hand
//!    (`npc::witness::update_crime_log`).
//! 2. **Witnesses** — NPCs whose AI tick sees the crime within their detect
//!    radius/LoS (`npc::systems::resolve_witnessed_crime`).
//! 3. **Gossip** — NPCs that know spread relevant records to nearby
//!    faction-bearing NPCs ([`tick_crime_gossip`]).
//!
//! Effects are capped into tiers ([`GuiltTier`]) even though the point total
//! is not: past `SHUNNED_THRESHOLD` an NPC refuses to talk or trade, past
//! `WANTED_THRESHOLD` it attacks on sight (if it has combat AI at all).
//! Records are settled per crime at a [`Judge`] — paying removes that crime id
//! from *every* NPC's memory at once (justice is official and public).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::npc::hostility::{TagInterner, TagMask};
use crate::player::components::{ChatLog, PlayerId, PlayerIdentity};

/// Interned social factions. Same bit-per-string representation as
/// [`TagMask`], but a separate interner (and therefore a separate 64-entry
/// budget) so allegiances and identity tags never crowd each other out.
pub type FactionMask = TagMask;

/// Guilt points for a single (debounced) assault on a faction member.
pub const ATTACK_GUILT: u32 = 10;

/// Guilt points for killing a faction member. Deliberately clear of
/// [`WANTED_THRESHOLD`] on its own: one witnessed murder makes you a wanted
/// criminal outright, with no accumulation needed.
pub const KILL_GUILT: u32 = 70;

/// Repeated hits on the *same* victim inside this window fold into one crime
/// record, so a damage-over-time tick or a flurry of fast swings doesn't
/// ratchet a player to Wanted for what is, narratively, one attack.
pub const ATTACK_DEBOUNCE_SECONDS: f32 = 3.0;

/// Guilt at or above this refuses conversation and trade.
pub const SHUNNED_THRESHOLD: u32 = 31;

/// Guilt at or above this is hostile on sight.
pub const WANTED_THRESHOLD: u32 = 61;

/// How far (Chebyshev, tiles) crime gossip carries between NPCs. Deliberately
/// short — it models talking, not shouting. `[tunable]`
pub const GOSSIP_RADIUS_TILES: i32 = 3;

/// How often the gossip sweep runs. `[tunable]`
pub const GOSSIP_INTERVAL_SECONDS: f32 = 4.0;

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
/// build/resolve are exactly right) and adds the reverse lookups chat lines
/// and persisted crime records need.
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

    /// Raw faction ids for every faction in `mask` — the inverse of
    /// [`Self::resolve`]. Used to stringify a mask into a [`CrimeRecord`],
    /// which persists across runs while the interner's bit assignment does not.
    pub fn names_for_mask(&self, mask: FactionMask) -> Vec<String> {
        mask.bits()
            .filter_map(|bit| self.inner.name_for_bit(bit))
            .map(str::to_owned)
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

/// An NPC who will, for coin, settle a player's crimes against the factions it
/// speaks for. Derived at spawn from the definition's `judge:` block, like
/// [`FactionMembership`] — the fee schedule is template data, never persisted.
#[derive(Component, Clone, Copy, Debug)]
pub struct Judge {
    /// Factions this Judge has the authority to absolve.
    pub clears: FactionMask,
    /// Fee in copper per guilt point of the crime being settled.
    pub copper_per_guilt_point: u32,
}

/// What was done. Point value is derived, never stored, so a rebalance of the
/// constants retroactively reprices old saves.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CrimeKind {
    Attack,
    Kill,
}

impl CrimeKind {
    pub fn points(self) -> u32 {
        match self {
            CrimeKind::Attack => ATTACK_GUILT,
            CrimeKind::Kill => KILL_GUILT,
        }
    }

    /// "Assault on Bob" / "Murder of Bob" — the judge-ledger description stem.
    pub fn describe(self, victim_name: &str) -> String {
        match self {
            CrimeKind::Attack => format!("Assault on {victim_name}"),
            CrimeKind::Kill => format!("Murder of {victim_name}"),
        }
    }
}

/// One specific crime, minted in `npc::witness::update_crime_log` when an
/// attributed hit lands on a faction-bearing NPC. The same record (same `id`)
/// is copied into every NPC that comes to know of it, so settling the crime
/// can find and remove every copy.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CrimeRecord {
    /// Globally unique, from [`CrimeIdAllocator`]; monotonic, so sorting by id
    /// is chronological.
    pub id: u64,
    /// The perpetrator. Safe to persist: `PlayerId` is the character id, stable
    /// across saves (object ids are reallocated on load and must never be
    /// keyed on here).
    pub player: PlayerId,
    pub kind: CrimeKind,
    /// The victim's display name at crime time, for the judge ledger.
    pub victim_name: String,
    /// Raw faction id strings (`emberbrook_watch`), **not** a [`FactionMask`]:
    /// the interner's bit assignment is rebuilt from YAML every boot and is
    /// not stable across content edits, and this record is persisted. Resolve
    /// on demand via [`FactionInterner::resolve`].
    pub victim_factions: Vec<String>,
}

/// The specific crimes this NPC knows about.
///
/// Stored as a `Vec` rather than a map on purpose: it is empty for almost
/// every NPC and never holds more than a handful of records, and a `Vec`
/// round-trips through `serde_json` without a custom key codec.
///
/// **Persisted** in `NpcStateDump` — unlike the template-derived components,
/// this is runtime state and would silently reset on every world reload
/// otherwise.
#[derive(Component, Clone, Debug, Default, Deserialize, Serialize)]
pub struct CrimeMemory {
    #[serde(default)]
    pub records: Vec<CrimeRecord>,
}

impl CrimeMemory {
    /// Total guilt points this NPC holds against `player`.
    pub fn points(&self, player: PlayerId) -> u32 {
        self.records
            .iter()
            .filter(|r| r.player == player)
            .map(|r| r.kind.points())
            .fold(0u32, u32::saturating_add)
    }

    pub fn tier(&self, player: PlayerId) -> GuiltTier {
        GuiltTier::of(self.points(player))
    }

    /// Whether this NPC already knows `record` at its current (or greater)
    /// severity. An Attack copy of a crime later upgraded to Kill does *not*
    /// count as known, so witnesses re-learn the worse truth.
    pub fn knows_at_least(&self, record: &CrimeRecord) -> bool {
        self.records
            .iter()
            .any(|r| r.id == record.id && r.kind.points() >= record.kind.points())
    }

    /// Add `record`, deduping by id. A copy with higher points (Attack →
    /// Kill upgrade) replaces the stale one. Returns whether anything changed.
    pub fn learn(&mut self, record: &CrimeRecord) -> bool {
        match self.records.iter_mut().find(|r| r.id == record.id) {
            Some(existing) => {
                if record.kind.points() > existing.kind.points() {
                    *existing = record.clone();
                    true
                } else {
                    false
                }
            }
            None => {
                self.records.push(record.clone());
                true
            }
        }
    }

    /// Remove one settled crime, wherever it came from.
    pub fn clear_crime(&mut self, id: u64) {
        self.records.retain(|r| r.id != id);
    }

    /// Drop every record of `player` wronging a faction in `mask` — the
    /// death-by-faction forgiveness path.
    pub fn clear_player_factions(
        &mut self,
        player: PlayerId,
        mask: FactionMask,
        interner: &FactionInterner,
    ) {
        self.records.retain(|r| {
            r.player != player || !interner.resolve(&r.victim_factions).intersects(mask)
        });
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Test fixture: a memory pre-loaded with one crime per entry of `kinds`,
    /// all by `player` against an unspecified faction.
    #[cfg(test)]
    pub(crate) fn test_knowing(player: PlayerId, kinds: &[CrimeKind]) -> Self {
        let mut memory = Self::default();
        for (index, kind) in kinds.iter().enumerate() {
            memory.learn(&CrimeRecord {
                id: index as u64 + 1,
                player,
                kind: *kind,
                victim_name: "Someone".to_owned(),
                victim_factions: vec!["test_faction".to_owned()],
            });
        }
        memory
    }
}

/// Hands out globally unique [`CrimeRecord`] ids. Persisted in
/// `WorldStateDump` so a reload can never reissue an id that lives in some
/// NPC's saved memory (settling the new crime would silently pardon the old).
#[derive(Resource, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct CrimeIdAllocator {
    pub next: u64,
}

impl Default for CrimeIdAllocator {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl CrimeIdAllocator {
    pub fn allocate(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
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
pub fn refuses_interaction(guilt: Option<&CrimeMemory>, player: PlayerId) -> bool {
    guilt.is_some_and(|g| g.tier(player) >= GuiltTier::Shunned)
}

/// One NPC coming to know of one crime — from first-hand experience (the
/// victim), from witnessing it, or from gossip. Drained by
/// [`apply_crime_memory_updates`].
#[derive(Resource, Default)]
pub struct PendingCrimeLearns {
    pub items: Vec<(Entity, CrimeRecord)>,
}

impl PendingCrimeLearns {
    pub fn push(&mut self, learner: Entity, record: CrimeRecord) {
        self.items.push((learner, record));
    }
}

/// A request to erase guilt, drained by [`apply_crime_memory_updates`].
#[derive(Clone, Copy, Debug)]
pub enum GuiltClear {
    /// The sentence was carried out: dying to a faction member clears every
    /// crime of `player` against that faction, on every NPC.
    Faction {
        player: PlayerId,
        factions: FactionMask,
    },
    /// A fine was paid at a Judge: the crime is settled, publicly and
    /// world-wide — the id vanishes from every NPC's memory at once.
    Crime { id: u64 },
}

#[derive(Resource, Default)]
pub struct PendingGuiltClears {
    pub items: Vec<GuiltClear>,
}

impl PendingGuiltClears {
    pub fn push(&mut self, clear: GuiltClear) {
        self.items.push(clear);
    }
}

/// Drains [`PendingCrimeLearns`] and [`PendingGuiltClears`] into the NPCs'
/// [`CrimeMemory`] components, and narrates the player's *effective* standing
/// changes.
///
/// Narration mirrors the old faction-broadcast behavior from the player's
/// perspective: the line fires when the **worst tier held by any member of the
/// wronged faction** crosses a boundary — i.e. when the first witness (or the
/// surviving victim) learns enough to change how the faction can treat you —
/// and stays silent as gossip spreads the same record further.
pub fn apply_crime_memory_updates(
    mut pending_learns: ResMut<PendingCrimeLearns>,
    mut pending_clears: ResMut<PendingGuiltClears>,
    interner: Res<FactionInterner>,
    mut memories: Query<(Entity, Option<&FactionMembership>, Option<&mut CrimeMemory>)>,
    mut players: Query<(&PlayerIdentity, &mut ChatLog)>,
    mut commands: Commands,
) {
    if pending_learns.items.is_empty() && pending_clears.items.is_empty() {
        return;
    }
    let learns = std::mem::take(&mut pending_learns.items);
    let clears = std::mem::take(&mut pending_clears.items);

    // The (player, faction-mask) pairs whose effective standing this drain can
    // change, for the before/after narration sweep.
    let mut affected: Vec<(PlayerId, FactionMask)> = Vec::new();
    let mut note_affected = |player: PlayerId, mask: FactionMask| {
        if !mask.is_empty() && !affected.iter().any(|(p, m)| *p == player && *m == mask) {
            affected.push((player, mask));
        }
    };
    for (_, record) in &learns {
        note_affected(record.player, interner.resolve(&record.victim_factions));
    }
    for clear in &clears {
        match clear {
            GuiltClear::Faction { player, factions } => note_affected(*player, *factions),
            GuiltClear::Crime { id } => {
                // Resolve the settled crime's player/factions from any live
                // copy, for narration.
                if let Some(record) = memories
                    .iter()
                    .filter_map(|(_, _, memory)| memory)
                    .flat_map(|m| m.records.iter())
                    .find(|r| r.id == *id)
                {
                    note_affected(record.player, interner.resolve(&record.victim_factions));
                }
            }
        }
    }

    // Effective standing before: worst tier across live faction members.
    let tier_toward =
        |memories: &Query<(Entity, Option<&FactionMembership>, Option<&mut CrimeMemory>)>,
         fresh: &HashMap<Entity, CrimeMemory>,
         player: PlayerId,
         mask: FactionMask| {
            memories
                .iter()
                .filter(|(_, membership, _)| membership.is_some_and(|m| m.mask.intersects(mask)))
                .map(|(entity, _, memory)| match fresh.get(&entity) {
                    Some(pending) => pending.tier(player),
                    None => memory.map(|m| m.tier(player)).unwrap_or_default(),
                })
                .max()
                .unwrap_or_default()
        };
    let no_fresh: HashMap<Entity, CrimeMemory> = HashMap::new();
    let before: Vec<GuiltTier> = affected
        .iter()
        .map(|(player, mask)| tier_toward(&memories, &no_fresh, *player, *mask))
        .collect();

    // Clears first: a pardon and a fresh offense in one frame should leave the
    // fresh offense standing.
    if !clears.is_empty() {
        for (_, _, memory) in &mut memories {
            let Some(mut memory) = memory else { continue };
            for clear in &clears {
                match clear {
                    GuiltClear::Faction { player, factions } => {
                        memory.clear_player_factions(*player, *factions, &interner);
                    }
                    GuiltClear::Crime { id } => memory.clear_crime(*id),
                }
            }
        }
    }

    // Learns, grouped per learner: an entity without a `CrimeMemory` gets a
    // fresh one via `Commands` (attached lazily rather than pre-seeding every
    // NPC with an empty component). The fresh map keeps this frame's inserts
    // visible to the after-tier sweep below, since commands apply later.
    let mut fresh: HashMap<Entity, CrimeMemory> = HashMap::new();
    for (learner, record) in learns {
        let Ok((_, _, memory)) = memories.get_mut(learner) else {
            continue;
        };
        match memory {
            Some(mut memory) => {
                memory.learn(&record);
            }
            None => {
                fresh.entry(learner).or_default().learn(&record);
            }
        }
    }
    for (entity, memory) in &fresh {
        commands.entity(*entity).insert(memory.clone());
    }

    for ((player, mask), before) in affected.into_iter().zip(before) {
        let after = tier_toward(&memories, &fresh, player, mask);
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
        let names = interner.display_names(mask);
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

/// The union of `player`'s crimes known to any live member of the factions in
/// `clears`, filtered to crimes *against* those factions, deduped by id
/// (keeping the worst copy) and sorted chronologically. This is what a Judge
/// lists and prices.
pub fn crimes_owed_to_judge(
    members: &Query<(&FactionMembership, &CrimeMemory)>,
    interner: &FactionInterner,
    clears: FactionMask,
    player: PlayerId,
) -> Vec<CrimeRecord> {
    let mut union: Vec<CrimeRecord> = Vec::new();
    for (membership, memory) in members.iter() {
        if !membership.mask.intersects(clears) {
            continue;
        }
        for record in &memory.records {
            if record.player != player
                || !interner.resolve(&record.victim_factions).intersects(clears)
            {
                continue;
            }
            match union.iter_mut().find(|r| r.id == record.id) {
                Some(existing) => {
                    if record.kind.points() > existing.kind.points() {
                        *existing = record.clone();
                    }
                }
                None => union.push(record.clone()),
            }
        }
    }
    union.sort_by_key(|r| r.id);
    union
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

/// What a judge command wants done, after the shared claim.
enum JudgeRequest {
    List,
    Pay { crime_id: u64 },
}

/// Compose the ledger rows the client renders, priced by this judge.
fn compose_listings(
    owed: &[CrimeRecord],
    judge: &Judge,
) -> Vec<crate::game::resources::CrimeListing> {
    owed.iter()
        .map(|record| crate::game::resources::CrimeListing {
            crime_id: record.id,
            description: record.kind.describe(&record.victim_name),
            price_text: crate::game::currency::format_compact(
                record
                    .kind
                    .points()
                    .saturating_mul(judge.copper_per_guilt_point),
            ),
        })
        .collect()
}

/// Handles `GameCommand::RequestCrimeList` and `GameCommand::PayCrime`: the
/// per-crime judge flow. Listing sends the ledger of the player's known crimes
/// against the judge's factions (`GameUiEvent::OpenCrimeLedger`); paying
/// settles one crime — coin out, the crime id erased from every NPC's memory —
/// and re-sends the shrunken ledger so the window refreshes.
///
/// Drained out of `PendingGameCommands` in the `CommandIntercept` set, the same
/// way dialog and trade claim their own commands, rather than growing the
/// already-enormous `process_game_commands` match.
pub fn process_judge_commands(
    mut pending_commands: ResMut<crate::game::resources::PendingGameCommands>,
    mut players: JudgePlayerQuery,
    judges: Query<(
        &crate::world::components::OverworldObject,
        &crate::world::components::SpaceResident,
        &crate::world::components::TilePosition,
        &Judge,
    )>,
    members: Query<(&FactionMembership, &CrimeMemory)>,
    definitions: Res<crate::world::object_definitions::OverworldObjectDefinitions>,
    spell_definitions: Res<crate::magic::resources::SpellDefinitions>,
    object_registry: Res<crate::world::object_registry::ObjectRegistry>,
    floors: crate::world::column::FloorGeometryParam,
    interner: Res<FactionInterner>,
    mut pending: ResMut<PendingGuiltClears>,
    mut ui_events: ResMut<crate::game::resources::PendingGameUiEvents>,
) {
    // Cheap read-only probe first. `drain_matching` rebuilds the whole queue
    // and marks the resource changed, which is wasteful to do every frame for a
    // command a player issues once in a blue moon — read through `Deref` and
    // only take the queue when there is actually something of ours in it.
    let has_ours = pending_commands.commands.iter().any(|queued| {
        matches!(
            queued.command,
            crate::game::commands::GameCommand::RequestCrimeList { .. }
                | crate::game::commands::GameCommand::PayCrime { .. }
        )
    });
    if !has_ours {
        return;
    }
    let claimed = pending_commands.drain_matching(|command| match command {
        crate::game::commands::GameCommand::RequestCrimeList { npc_object_id } => {
            Ok((npc_object_id, JudgeRequest::List))
        }
        crate::game::commands::GameCommand::PayCrime {
            npc_object_id,
            crime_id,
        } => Ok((npc_object_id, JudgeRequest::Pay { crime_id })),
        other => Err(other),
    });
    if claimed.is_empty() {
        return;
    }
    let geometry = floors.geometry();
    // Crimes settled by earlier commands in this same drain: the memory sweep
    // only applies the clears later this frame, so re-derived ledgers (and a
    // double-clicked Pay) must exclude them by hand.
    let mut settled_this_frame: Vec<u64> = Vec::new();

    for (queued_player_id, (npc_object_id, request)) in claimed {
        let Some(acting_player_id) = queued_player_id else {
            continue;
        };
        let Some((judge_object, judge_resident, judge_tile, judge)) = judges
            .iter()
            .find(|(object, _, _, _)| object.object_id == npc_object_id)
        else {
            crate::game::helpers::refuse(acting_player_id, "JudgeCommand", "not a judge");
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
            crate::game::helpers::refuse(acting_player_id, "JudgeCommand", "judge out of range");
            continue;
        }

        let mut owed = crimes_owed_to_judge(&members, &interner, judge.clears, acting_player_id);
        owed.retain(|record| !settled_this_frame.contains(&record.id));
        let judge_name = object_registry
            .display_name(judge_object.object_id, &definitions, &spell_definitions)
            .unwrap_or_else(|| "Judge".to_owned());

        match request {
            JudgeRequest::List => {
                if owed.is_empty() {
                    chat.push_narrator("The judge waves you off. \"You owe nothing here.\"");
                    continue;
                }
                ui_events.push(
                    acting_player_id,
                    crate::game::resources::GameUiEvent::OpenCrimeLedger {
                        npc_object_id,
                        judge_name,
                        crimes: compose_listings(&owed, judge),
                    },
                );
            }
            JudgeRequest::Pay { crime_id } => {
                // Server-authoritative: the id must be in the freshly derived
                // ledger, whatever the client claimed to be showing.
                let Some(record) = owed.iter().find(|r| r.id == crime_id).cloned() else {
                    crate::game::helpers::refuse(
                        acting_player_id,
                        "PayCrime",
                        "crime not on this judge's ledger",
                    );
                    continue;
                };
                let fee = record
                    .kind
                    .points()
                    .saturating_mul(judge.copper_per_guilt_point);
                if !crate::game::currency::spend_copper(&mut inventory, fee, &definitions) {
                    chat.push_narrator(format!(
                        "The judge shakes their head. \"That one costs {} — come back when you can pay it.\"",
                        crate::game::currency::format_compact(fee)
                    ));
                    continue;
                }

                settled_this_frame.push(crime_id);
                pending.push(GuiltClear::Crime { id: crime_id });
                chat.push_narrator(format!(
                    "You pay {} and the matter of {} is settled.",
                    crate::game::currency::format_compact(fee),
                    record.kind.describe(&record.victim_name),
                ));
                // Refresh the open window with what remains (empty ⇒ the
                // client closes it).
                owed.retain(|r| r.id != crime_id);
                ui_events.push(
                    acting_player_id,
                    crate::game::resources::GameUiEvent::OpenCrimeLedger {
                        npc_object_id,
                        judge_name,
                        crimes: compose_listings(&owed, judge),
                    },
                );
            }
        }
    }
}

/// Spreads crime knowledge between NPCs standing near each other: every
/// [`GOSSIP_INTERVAL_SECONDS`], each NPC that knows of a crime shares it with
/// faction-bearing NPCs in the same space, on the same floor, within
/// [`GOSSIP_RADIUS_TILES`] — but a listener only *retains* records relevant to
/// it (victim factions intersecting its membership or its `Protector` remit),
/// so a goblin never carries a grudge about a murdered guard.
///
/// No line-of-sight check: this models talk, not sight. A future per-template
/// `gossips: bool` opt-out would simply filter the source query here.
pub fn tick_crime_gossip(
    time: Res<Time>,
    mut accumulator: Local<f32>,
    interner: Res<FactionInterner>,
    sources: Query<
        (
            Entity,
            &crate::world::components::SpaceResident,
            &crate::world::components::TilePosition,
            &CrimeMemory,
        ),
        With<crate::npc::components::Npc>,
    >,
    listeners: Query<
        (
            Entity,
            &crate::world::components::SpaceResident,
            &crate::world::components::TilePosition,
            &FactionMembership,
            Option<&crate::npc::witness::Protector>,
            Option<&CrimeMemory>,
        ),
        With<crate::npc::components::Npc>,
    >,
    mut learns: ResMut<PendingCrimeLearns>,
) {
    *accumulator += time.delta_secs();
    if *accumulator < GOSSIP_INTERVAL_SECONDS {
        return;
    }
    *accumulator = 0.0;

    // The overwhelmingly common steady state: nobody knows anything.
    if sources.iter().all(|(_, _, _, memory)| memory.is_empty()) {
        return;
    }

    use crate::world::components::floor_index;
    for (teller, teller_resident, teller_tile, memory) in &sources {
        if memory.is_empty() {
            continue;
        }
        for (listener, resident, tile, membership, protector, listener_memory) in &listeners {
            if listener == teller
                || resident.space_id != teller_resident.space_id
                || floor_index(tile.z) != floor_index(teller_tile.z)
            {
                continue;
            }
            let dx = (tile.x - teller_tile.x).abs();
            let dy = (tile.y - teller_tile.y).abs();
            if dx.max(dy) > GOSSIP_RADIUS_TILES {
                continue;
            }
            let remit = protector.map(|p| p.protects).unwrap_or_default();
            for record in &memory.records {
                let victim_mask = interner.resolve(&record.victim_factions);
                if !victim_mask.intersects(membership.mask) && !victim_mask.intersects(remit) {
                    continue;
                }
                if listener_memory.is_some_and(|m| m.knows_at_least(record)) {
                    continue;
                }
                learns.push(listener, record.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::components::{SpaceId, SpaceResident, TilePosition};

    fn mask(bits: &[u8]) -> FactionMask {
        TagMask(bits.iter().fold(0u64, |acc, b| acc | (1 << b)))
    }

    const WATCH: u8 = 1;

    fn test_interner() -> FactionInterner {
        // TagInterner assigns bits in iteration order starting at... build a
        // real one and resolve by name instead of assuming bit positions.
        FactionInterner::build(["emberbrook_watch", "goblin_tribe"].into_iter())
    }

    fn record(id: u64, player: PlayerId, kind: CrimeKind, factions: &[&str]) -> CrimeRecord {
        CrimeRecord {
            id,
            player,
            kind,
            victim_name: "Bob".to_owned(),
            victim_factions: factions.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn watch_record(id: u64, player: PlayerId, kind: CrimeKind) -> CrimeRecord {
        record(id, player, kind, &["emberbrook_watch"])
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<PendingCrimeLearns>();
        app.init_resource::<PendingGuiltClears>();
        app.insert_resource(test_interner());
        app.add_systems(Update, apply_crime_memory_updates);
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

    fn watch_mask(app: &App) -> FactionMask {
        app.world()
            .resource::<FactionInterner>()
            .resolve(&["emberbrook_watch".to_owned()])
    }

    fn tribe_mask(app: &App) -> FactionMask {
        app.world()
            .resource::<FactionInterner>()
            .resolve(&["goblin_tribe".to_owned()])
    }

    fn guilt_of(app: &App, entity: Entity, player: PlayerId) -> u32 {
        app.world()
            .get::<CrimeMemory>(entity)
            .map(|g| g.points(player))
            .unwrap_or(0)
    }

    fn push_learn(app: &mut App, learner: Entity, record: CrimeRecord) {
        app.world_mut()
            .resource_mut::<PendingCrimeLearns>()
            .push(learner, record);
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
    fn a_learn_reaches_only_the_learner() {
        let mut app = test_app();
        let player = PlayerId(7);
        spawn_player(&mut app, player);
        let witness = spawn_member(&mut app, mask(&[WATCH]));
        let elsewhere = spawn_member(&mut app, mask(&[WATCH]));

        push_learn(&mut app, witness, watch_record(1, player, CrimeKind::Kill));
        app.update();

        assert_eq!(guilt_of(&app, witness, player), KILL_GUILT);
        assert_eq!(
            guilt_of(&app, elsewhere, player),
            0,
            "guilt is per-NPC: only the entity that learned holds the record"
        );
    }

    #[test]
    fn a_single_kill_record_reaches_wanted() {
        let mut app = test_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        push_learn(&mut app, guard, watch_record(1, player, CrimeKind::Kill));
        app.update();

        assert_eq!(
            app.world().get::<CrimeMemory>(guard).unwrap().tier(player),
            GuiltTier::Wanted
        );
    }

    #[test]
    fn a_single_attack_record_stays_clean_four_reach_shunned() {
        let mut app = test_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        push_learn(&mut app, guard, watch_record(1, player, CrimeKind::Attack));
        app.update();
        assert_eq!(
            app.world().get::<CrimeMemory>(guard).unwrap().tier(player),
            GuiltTier::Clean
        );

        for id in 2..=4 {
            push_learn(&mut app, guard, watch_record(id, player, CrimeKind::Attack));
        }
        app.update();
        assert_eq!(guilt_of(&app, guard, player), ATTACK_GUILT * 4);
        assert_eq!(
            app.world().get::<CrimeMemory>(guard).unwrap().tier(player),
            GuiltTier::Shunned,
            "a brawl gets you shunned; it takes a killing to be hunted"
        );
    }

    #[test]
    fn learning_the_same_crime_twice_charges_once() {
        let mut app = test_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        push_learn(&mut app, guard, watch_record(1, player, CrimeKind::Attack));
        app.update();
        push_learn(&mut app, guard, watch_record(1, player, CrimeKind::Attack));
        app.update();

        assert_eq!(
            guilt_of(&app, guard, player),
            ATTACK_GUILT,
            "the same record heard twice (witness + gossip) is one crime"
        );
    }

    #[test]
    fn a_kill_upgrade_replaces_the_attack_copy() {
        let mut app = test_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        push_learn(&mut app, guard, watch_record(1, player, CrimeKind::Attack));
        app.update();
        push_learn(&mut app, guard, watch_record(1, player, CrimeKind::Kill));
        app.update();

        assert_eq!(
            guilt_of(&app, guard, player),
            KILL_GUILT,
            "murder subsumes the assault it grew out of — same id, worse kind"
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

        push_learn(&mut app, guard, watch_record(1, culprit, CrimeKind::Kill));
        app.update();

        assert_eq!(guilt_of(&app, guard, culprit), KILL_GUILT);
        assert_eq!(guilt_of(&app, guard, innocent), 0);
    }

    #[test]
    fn faction_clear_drops_only_that_factions_records() {
        let mut app = test_app();
        let player = PlayerId(3);
        spawn_player(&mut app, player);
        let guard = spawn_member(&mut app, mask(&[WATCH]));

        push_learn(&mut app, guard, watch_record(1, player, CrimeKind::Kill));
        push_learn(
            &mut app,
            guard,
            record(2, player, CrimeKind::Kill, &["goblin_tribe"]),
        );
        app.update();
        assert_eq!(guilt_of(&app, guard, player), KILL_GUILT * 2);

        let watch = watch_mask(&app);
        app.world_mut()
            .resource_mut::<PendingGuiltClears>()
            .push(GuiltClear::Faction {
                player,
                factions: watch,
            });
        app.update();

        assert_eq!(
            guilt_of(&app, guard, player),
            KILL_GUILT,
            "settling with one faction must not absolve crimes against another"
        );
    }

    #[test]
    fn crime_clear_is_global() {
        let mut app = test_app();
        let player = PlayerId(3);
        spawn_player(&mut app, player);
        let witness = spawn_member(&mut app, mask(&[WATCH]));
        let gossiped = spawn_member(&mut app, mask(&[WATCH]));

        push_learn(&mut app, witness, watch_record(1, player, CrimeKind::Kill));
        push_learn(&mut app, gossiped, watch_record(1, player, CrimeKind::Kill));
        push_learn(
            &mut app,
            witness,
            watch_record(2, player, CrimeKind::Attack),
        );
        app.update();

        app.world_mut()
            .resource_mut::<PendingGuiltClears>()
            .push(GuiltClear::Crime { id: 1 });
        app.update();

        assert_eq!(guilt_of(&app, witness, player), ATTACK_GUILT);
        assert_eq!(
            guilt_of(&app, gossiped, player),
            0,
            "settling a crime erases every copy, however it was learned"
        );
    }

    #[test]
    fn crime_memory_round_trips_through_json() {
        // A `Vec` rather than a map precisely so serde_json needs no custom
        // key codec — guard that.
        let mut memory = CrimeMemory::default();
        memory.learn(&watch_record(1, PlayerId(42), CrimeKind::Kill));
        memory.learn(&watch_record(2, PlayerId(7), CrimeKind::Attack));

        let json = serde_json::to_string(&memory).unwrap();
        let restored: CrimeMemory = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.points(PlayerId(42)), KILL_GUILT);
        assert_eq!(restored.points(PlayerId(7)), ATTACK_GUILT);
        assert_eq!(restored.points(PlayerId(99)), 0);
        assert_eq!(restored.records[0].victim_name, "Bob");

        // Rows written before the field existed must still load.
        let legacy: CrimeMemory = serde_json::from_str("{}").unwrap();
        assert!(legacy.is_empty());
    }

    #[test]
    fn many_records_accumulate_uncapped() {
        // The point total is deliberately uncapped even though the *effects*
        // cap at Wanted — a serial killer's debt keeps growing, and so does
        // the fine.
        let mut memory = CrimeMemory::default();
        for id in 0..10 {
            memory.learn(&watch_record(id, PlayerId(1), CrimeKind::Kill));
        }
        assert_eq!(memory.points(PlayerId(1)), KILL_GUILT * 10);
        assert_eq!(memory.tier(PlayerId(1)), GuiltTier::Wanted);
    }

    #[test]
    fn faction_ids_prettify_for_player_facing_text() {
        assert_eq!(prettify_faction_id("emberbrook_watch"), "Emberbrook Watch");
        assert_eq!(prettify_faction_id("watch"), "Watch");
        assert_eq!(prettify_faction_id(""), "");
    }

    #[test]
    fn interner_round_trips_names_through_the_mask() {
        let interner = test_interner();
        let watch = interner.resolve(&["emberbrook_watch".to_owned()]);
        assert!(!watch.is_empty());
        assert_eq!(interner.display_names(watch), vec!["Emberbrook Watch"]);
        assert_eq!(interner.names_for_mask(watch), vec!["emberbrook_watch"]);
        // Unknown factions resolve to nothing rather than panicking, matching
        // the tag interner.
        assert!(interner.resolve(&["nonexistent".to_owned()]).is_empty());
    }

    // ------------------------------------------------------------------
    // Gossip
    // ------------------------------------------------------------------

    fn gossip_app() -> App {
        let mut app = test_app();
        app.add_systems(Update, tick_crime_gossip.before(apply_crime_memory_updates));
        // First update initializes Time.
        app.update();
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(std::time::Duration::MAX);
        app
    }

    fn advance(app: &mut App, seconds: f32) {
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f32(seconds),
        ));
        app.update();
    }

    fn spawn_gossip_npc(
        app: &mut App,
        factions: FactionMask,
        tile: (i32, i32),
        space: SpaceId,
    ) -> Entity {
        app.world_mut()
            .spawn((
                crate::npc::components::Npc,
                FactionMembership { mask: factions },
                SpaceResident { space_id: space },
                TilePosition::ground(tile.0, tile.1),
            ))
            .id()
    }

    fn seed_memory(app: &mut App, entity: Entity, record: CrimeRecord) {
        let mut memory = CrimeMemory::default();
        memory.learn(&record);
        app.world_mut().entity_mut(entity).insert(memory);
    }

    #[test]
    fn gossip_spreads_to_nearby_relevant_npcs_only() {
        let mut app = gossip_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let space = SpaceId(1);
        let watch = watch_mask(&app);
        let tribe = tribe_mask(&app);

        let teller = spawn_gossip_npc(&mut app, watch, (5, 5), space);
        let near_mate = spawn_gossip_npc(&mut app, watch, (7, 5), space);
        let far_mate = spawn_gossip_npc(&mut app, watch, (15, 5), space);
        let other_space = spawn_gossip_npc(&mut app, watch, (5, 5), SpaceId(2));
        let goblin = spawn_gossip_npc(&mut app, tribe, (6, 5), space);
        seed_memory(&mut app, teller, watch_record(1, player, CrimeKind::Kill));

        advance(&mut app, GOSSIP_INTERVAL_SECONDS + 0.1);

        assert_eq!(guilt_of(&app, near_mate, player), KILL_GUILT);
        assert_eq!(guilt_of(&app, far_mate, player), 0, "3-tile radius");
        assert_eq!(guilt_of(&app, other_space, player), 0, "same space only");
        assert_eq!(
            guilt_of(&app, goblin, player),
            0,
            "irrelevant faction never retains the record"
        );
    }

    #[test]
    fn gossip_waits_for_the_interval_and_does_not_duplicate() {
        let mut app = gossip_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let space = SpaceId(1);
        let watch = watch_mask(&app);

        let teller = spawn_gossip_npc(&mut app, watch, (5, 5), space);
        let listener = spawn_gossip_npc(&mut app, watch, (6, 5), space);
        seed_memory(&mut app, teller, watch_record(1, player, CrimeKind::Attack));

        advance(&mut app, GOSSIP_INTERVAL_SECONDS * 0.5);
        assert_eq!(guilt_of(&app, listener, player), 0, "not yet — 4s beat");

        advance(&mut app, GOSSIP_INTERVAL_SECONDS);
        assert_eq!(guilt_of(&app, listener, player), ATTACK_GUILT);

        // Several more beats: the record must not double-charge.
        advance(&mut app, GOSSIP_INTERVAL_SECONDS + 0.1);
        advance(&mut app, GOSSIP_INTERVAL_SECONDS + 0.1);
        assert_eq!(guilt_of(&app, listener, player), ATTACK_GUILT);
    }

    #[test]
    fn gossip_chains_across_hops() {
        // A learns a crime, B hears it from A, C later hears it from B.
        let mut app = gossip_app();
        let player = PlayerId(1);
        spawn_player(&mut app, player);
        let space = SpaceId(1);
        let watch = watch_mask(&app);

        let a = spawn_gossip_npc(&mut app, watch, (0, 0), space);
        let b = spawn_gossip_npc(&mut app, watch, (3, 0), space);
        let c = spawn_gossip_npc(&mut app, watch, (6, 0), space);
        seed_memory(&mut app, a, watch_record(1, player, CrimeKind::Kill));

        advance(&mut app, GOSSIP_INTERVAL_SECONDS + 0.1);
        assert_eq!(guilt_of(&app, b, player), KILL_GUILT);
        assert_eq!(guilt_of(&app, c, player), 0, "C is 6 tiles from A");

        advance(&mut app, GOSSIP_INTERVAL_SECONDS + 0.1);
        assert_eq!(guilt_of(&app, c, player), KILL_GUILT, "heard it from B");
    }

    // ------------------------------------------------------------------
    // Judge
    // ------------------------------------------------------------------

    /// App wired for the Judge command path: the judge handler plus the
    /// memory-update sweep it queues its clears into.
    fn judge_app() -> App {
        let mut app = test_app();
        app.init_resource::<crate::game::resources::PendingGameCommands>()
            .init_resource::<crate::game::resources::PendingGameUiEvents>()
            .init_resource::<crate::world::floor_map::FloorMaps>()
            .init_resource::<crate::world::object_registry::ObjectRegistry>()
            .insert_resource(
                crate::world::object_definitions::OverworldObjectDefinitions::load_from_disk(),
            )
            .insert_resource(crate::magic::resources::SpellDefinitions::load_from_disk())
            .insert_resource(
                crate::world::floor_definitions::FloorTilesetDefinitions::load_from_disk(),
            );
        // Judge handler first, so the clears it queues are swept the same frame.
        app.add_systems(
            Update,
            process_judge_commands.before(apply_crime_memory_updates),
        );
        app
    }

    const TEST_SPACE: SpaceId = SpaceId(0);

    fn spawn_judge(app: &mut App, object_id: u64, tile: (i32, i32), per_point: u32) -> Entity {
        let clears = watch_mask(app);
        app.world_mut()
            .spawn((
                crate::world::components::OverworldObject {
                    object_id,
                    definition_id: "judge".to_owned(),
                    placement_seq: 0,
                },
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(tile.0, tile.1),
                Judge {
                    clears,
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
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(tile.0, tile.1),
                inventory,
                ChatLog::default(),
            ))
            .id()
    }

    /// Spawn a watch member that already remembers `records`.
    fn spawn_knowing_member(app: &mut App, records: &[CrimeRecord]) -> Entity {
        let watch = watch_mask(app);
        let mut memory = CrimeMemory::default();
        for record in records {
            memory.learn(record);
        }
        app.world_mut()
            .spawn((FactionMembership { mask: watch }, memory))
            .id()
    }

    fn send_request_list(app: &mut App, player: PlayerId, npc_object_id: u64) {
        app.world_mut()
            .resource_mut::<crate::game::resources::PendingGameCommands>()
            .push_for_player(
                player,
                crate::game::commands::GameCommand::RequestCrimeList { npc_object_id },
            );
    }

    fn send_pay_crime(app: &mut App, player: PlayerId, npc_object_id: u64, crime_id: u64) {
        app.world_mut()
            .resource_mut::<crate::game::resources::PendingGameCommands>()
            .push_for_player(
                player,
                crate::game::commands::GameCommand::PayCrime {
                    npc_object_id,
                    crime_id,
                },
            );
    }

    fn purse_of(app: &App, entity: Entity) -> u32 {
        crate::game::currency::purse_total_copper(
            app.world()
                .get::<crate::player::components::Inventory>(entity)
                .unwrap(),
        )
    }

    /// The `OpenCrimeLedger` payloads queued for `player`, oldest first.
    fn ledgers_sent(
        app: &mut App,
        player: PlayerId,
    ) -> Vec<Vec<crate::game::resources::CrimeListing>> {
        app.world()
            .resource::<crate::game::resources::PendingGameUiEvents>()
            .peer_events
            .get(&player)
            .into_iter()
            .flatten()
            .filter_map(|event| match event {
                crate::game::resources::GameUiEvent::OpenCrimeLedger { crimes, .. } => {
                    Some(crimes.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn requesting_the_ledger_lists_the_union_of_known_crimes_with_prices() {
        let mut app = judge_app();
        let player = PlayerId(1);
        spawn_paying_player(&mut app, player, (5, 5), 3);
        spawn_judge(&mut app, 77, (5, 6), 4);
        // Two guards: one saw the murder and the assault, one only the
        // assault (via gossip). Also a crime against a faction this judge
        // doesn't speak for — it must not be listed.
        spawn_knowing_member(
            &mut app,
            &[
                watch_record(1, player, CrimeKind::Kill),
                watch_record(2, player, CrimeKind::Attack),
                record(3, player, CrimeKind::Kill, &["goblin_tribe"]),
            ],
        );
        spawn_knowing_member(&mut app, &[watch_record(2, player, CrimeKind::Attack)]);

        send_request_list(&mut app, player, 77);
        app.update();

        let ledgers = ledgers_sent(&mut app, player);
        assert_eq!(ledgers.len(), 1);
        let ledger = &ledgers[0];
        assert_eq!(ledger.len(), 2, "deduped union, judge's factions only");
        assert_eq!(ledger[0].crime_id, 1);
        assert_eq!(ledger[0].description, "Murder of Bob");
        assert_eq!(
            ledger[0].price_text,
            crate::game::currency::format_compact(KILL_GUILT * 4)
        );
        assert_eq!(ledger[1].crime_id, 2);
        assert_eq!(ledger[1].description, "Assault on Bob");
        assert_eq!(
            ledger[1].price_text,
            crate::game::currency::format_compact(ATTACK_GUILT * 4)
        );
    }

    #[test]
    fn paying_one_crime_settles_it_globally_and_leaves_the_rest() {
        let mut app = judge_app();
        let player = PlayerId(1);
        let player_entity = spawn_paying_player(&mut app, player, (5, 5), 3);
        spawn_judge(&mut app, 77, (5, 6), 4);
        let witness = spawn_knowing_member(
            &mut app,
            &[
                watch_record(1, player, CrimeKind::Kill),
                watch_record(2, player, CrimeKind::Attack),
            ],
        );
        let gossiped = spawn_knowing_member(&mut app, &[watch_record(1, player, CrimeKind::Kill)]);

        let purse_before = purse_of(&app, player_entity);
        send_pay_crime(&mut app, player, 77, 1);
        app.update();

        assert_eq!(
            guilt_of(&app, witness, player),
            ATTACK_GUILT,
            "only the paid crime is settled; the assault remains"
        );
        assert_eq!(
            guilt_of(&app, gossiped, player),
            0,
            "the pardon reaches every copy of the crime, however learned"
        );
        assert_eq!(
            purse_before - purse_of(&app, player_entity),
            KILL_GUILT * 4,
            "the fee is copper_per_guilt_point x that crime's points"
        );
        // The refreshed ledger shows what's left.
        let ledgers = ledgers_sent(&mut app, player);
        assert_eq!(ledgers.len(), 1);
        assert_eq!(ledgers[0].len(), 1);
        assert_eq!(ledgers[0][0].crime_id, 2);
    }

    #[test]
    fn paying_an_unlisted_crime_is_refused() {
        let mut app = judge_app();
        let player = PlayerId(1);
        let player_entity = spawn_paying_player(&mut app, player, (5, 5), 3);
        spawn_judge(&mut app, 77, (5, 6), 4);
        // The only known crime is against the goblins — not this judge's remit.
        let guard = spawn_knowing_member(
            &mut app,
            &[record(9, player, CrimeKind::Kill, &["goblin_tribe"])],
        );

        send_pay_crime(&mut app, player, 77, 9);
        app.update();

        assert_eq!(guilt_of(&app, guard, player), KILL_GUILT);
        assert_eq!(
            purse_of(&app, player_entity),
            3 * crate::game::currency::COPPER_PER_GOLD,
            "a judge cannot take coin for a crime outside its authority"
        );
    }

    #[test]
    fn a_player_who_cannot_pay_keeps_both_coin_and_guilt() {
        let mut app = judge_app();
        let player = PlayerId(1);
        // One gold (240c) against a 70x12 = 840c fine.
        let player_entity = spawn_paying_player(&mut app, player, (5, 5), 1);
        spawn_judge(&mut app, 77, (5, 6), 12);
        let guard = spawn_knowing_member(&mut app, &[watch_record(1, player, CrimeKind::Kill)]);

        send_pay_crime(&mut app, player, 77, 1);
        app.update();

        assert_eq!(
            guilt_of(&app, guard, player),
            KILL_GUILT,
            "an unaffordable fine must not clear guilt"
        );
        assert_eq!(
            purse_of(&app, player_entity),
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
        let guard = spawn_knowing_member(&mut app, &[watch_record(1, player, CrimeKind::Kill)]);

        send_pay_crime(&mut app, player, 77, 1);
        app.update();

        assert_eq!(guilt_of(&app, guard, player), KILL_GUILT);
        assert_eq!(
            purse_of(&app, player_entity),
            10 * crate::game::currency::COPPER_PER_GOLD,
            "shouting at a distant magistrate must not cost anything"
        );
        assert!(
            ledgers_sent(&mut app, player).is_empty(),
            "no ledger opens out of range either"
        );
    }

    #[test]
    fn the_judge_handler_leaves_other_commands_in_the_queue() {
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
                crate::game::commands::GameCommand::RequestCrimeList { npc_object_id: 77 },
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
}
