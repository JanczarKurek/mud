//! Server ECS → client view-state projection.
//!
//! Three pieces live here:
//! - [`compute_events_for_peer`] diffs the authoritative ECS against a supplied
//!   baseline and returns a `Vec<GameEvent>`. This is the single serializer
//!   for both embedded and networked clients — per-peer on the server, against
//!   the local `ClientGameState` in embedded mode.
//! - [`collect_game_events_from_authority`] is the embedded/server-local system
//!   wrapper that feeds `compute_events_for_peer` with the current
//!   `ClientGameState` resource as the baseline and writes the result into
//!   `PendingGameEvents`.
//! - [`apply_game_events_to_client_state`] folds pending events back into
//!   `ClientGameState`, keeping presentation in lock-step with authority.
//!
//! See the "EmbeddedClient Invariant" in `CLAUDE.md`: these functions are the
//! single fold through which all server → client state flows, both in
//! networked and embedded modes.

#[cfg(feature = "server-sim")]
use bevy::log::{debug, info};
use bevy::prelude::*;

#[cfg(feature = "server-sim")]
use crate::combat::components::{AttackProfile, CombatTarget};
#[cfg(feature = "server-sim")]
use crate::dialog::components::DialogNode;
#[cfg(feature = "server-sim")]
use crate::game::resources::{
    ChatLogState, ClientCarryWeight, ClientCombatStats, ClientExertion, ClientSpaceState,
    ClientVitalStats, ClientWorldObjectState, InventoryState, NpcAwareness, RegenBuffState,
};
#[cfg(feature = "server-sim")]
use crate::game::resources::{ClientActiveEffect, ClientRemotePlayerState};
use crate::game::resources::{ClientGameState, ClientStateRevisions, GameEvent, PendingGameEvents};
#[cfg(feature = "server-sim")]
use crate::game::shop::{Shopkeeper, StockMode, Stockpile};
#[cfg(feature = "server-sim")]
use crate::game::trade::{ActiveTrades, TradeParticipants, TradePartnerKind, WareView};
#[cfg(feature = "server-sim")]
use crate::magic::effects::MagicEffects;
#[cfg(feature = "server-sim")]
use crate::npc::components::Npc;
#[cfg(feature = "server-sim")]
use crate::player::classes::Class;
#[cfg(feature = "server-sim")]
use crate::player::components::{
    CurrentCarryWeight, DefenseStats, DerivedStats, DiscoveredTiles, Encumbered, MaxCarryWeight,
    Player, PlayerAppearance, PlayerId, PlayerIdentity, RegenBuffs, VitalStats, WeaponDamage,
};
#[cfg(feature = "server-sim")]
use crate::player::progression::{Experience, ExperienceView};
#[cfg(feature = "server-sim")]
use crate::player::skills::SkillSheet;
#[cfg(feature = "server-sim")]
use crate::world::components::{
    Container, Facing, Movable, ObjectState, OverworldObject, Quantity, Rotatable, SpaceId,
    SpacePosition, SpaceResident, TilePosition,
};
#[cfg(feature = "server-sim")]
use crate::world::floor_map::FloorMaps;
#[cfg(feature = "server-sim")]
use crate::world::lighting::{WorldClock, WORLD_TIME_EPSILON, WORLD_TIME_HEARTBEAT_SECS};
#[cfg(feature = "server-sim")]
use crate::world::object_definitions::OverworldObjectDefinitions;
#[cfg(feature = "server-sim")]
use crate::world::resources::SpaceManager;

/// Chebyshev (XY-square) tile radius around the local player within which
/// dynamic entities (remote players, containers, world objects) and per-tile
/// floor edits are replicated. `z` is deliberately ignored: entities on other
/// floors of the same space still replicate, so stairs/balcony sightlines
/// don't pop. Larger than [`crate::game::discovery::DISCOVERY_RADIUS`] so
/// entities exist on the client before they enter the visible fog disc; tweak
/// here to widen/narrow what each client receives.
pub const INTEREST_RADIUS: f32 = 30.0;

#[cfg(feature = "server-sim")]
fn in_interest_radius(local: TilePosition, other: TilePosition) -> bool {
    let dx = (local.x - other.x) as f32;
    let dy = (local.y - other.y) as f32;
    dx.abs() <= INTEREST_RADIUS && dy.abs() <= INTEREST_RADIUS
}

/// Push `$event` when the previous and projected values differ.
///
/// `$previous` and `$current` are compared with `!=`; when they differ,
/// `$event` is evaluated and pushed onto `$events`. Whether the pushed value
/// is cloned, copied, or moved is decided by the `$event` expression at the
/// call site; `Option` baselines are handled by wrapping `$current` in
/// `Some(..)` at the call site too.
#[cfg(feature = "server-sim")]
macro_rules! diff_emit {
    ($events:expr, $previous:expr, $current:expr, $event:expr $(,)?) => {
        if $previous != $current {
            $events.push($event);
        }
    };
}

#[cfg(feature = "server-sim")]
pub type ProjectionPlayerQuery<'w, 's> = Query<
    'w,
    's,
    (
        // Grouped to keep the outer tuple under Bevy's 15-tuple cap.
        (Entity, &'static PlayerIdentity),
        &'static InventoryState,
        &'static ChatLogState,
        // Optional: dead players are de-spatialized (components removed until
        // the respawn click) but must keep replicating their non-spatial state
        // (vitals, chat, inventory) so the death overlay works.
        Option<&'static SpaceResident>,
        Option<&'static TilePosition>,
        &'static VitalStats,
        &'static DerivedStats,
        Option<&'static CombatTarget>,
        &'static OverworldObject,
        Option<&'static Facing>,
        Option<&'static RegenBuffs>,
        (
            Option<&'static MaxCarryWeight>,
            Option<&'static CurrentCarryWeight>,
            Has<Encumbered>,
        ),
        Option<&'static Experience>,
        (
            Option<&'static Class>,
            Option<&'static crate::crafting::CharacterStash>,
            Option<&'static SkillSheet>,
            Option<&'static PlayerAppearance>,
        ),
        (
            Option<&'static MagicEffects>,
            &'static DefenseStats,
            &'static WeaponDamage,
            &'static AttackProfile,
            Option<&'static DiscoveredTiles>,
            Has<crate::player::components::Sneaking>,
            Has<crate::player::components::Aware>,
            Has<crate::player::components::AutoRetaliate>,
            Option<&'static crate::player::sense::SenseReveals>,
            Option<&'static crate::player::components::Exertion>,
        ),
    ),
    With<Player>,
>;

#[cfg(feature = "server-sim")]
pub type ProjectionObjectQuery<'w, 's> = Query<'w, 's, &'static OverworldObject>;

#[cfg(feature = "server-sim")]
pub type ProjectionWorldObjectQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static SpaceResident,
        &'static TilePosition,
        &'static OverworldObject,
        Option<&'static VitalStats>,
        Has<Container>,
        Has<Npc>,
        Has<Movable>,
        Has<Rotatable>,
        Option<&'static Quantity>,
        Has<DialogNode>,
        Option<&'static Facing>,
        Option<&'static ObjectState>,
        Has<Shopkeeper>,
        Option<&'static crate::world::hidden::Hidden>,
        // Grouped to keep the outer tuple under Bevy's 15-tuple cap.
        (
            Has<crate::npc::components::HostileBehavior>,
            Option<&'static CombatTarget>,
            Option<&'static crate::npc::components::AiState>,
            Option<&'static crate::npc::components::Faction>,
            Option<&'static crate::npc::hostility::TagProfile>,
            Option<&'static crate::npc::guilt::CrimeMemory>,
            Option<&'static crate::npc::guilt::Judge>,
        ),
    ),
    Without<Player>,
>;

#[cfg(feature = "server-sim")]
pub type ProjectionContainerQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Container,
        &'static OverworldObject,
        &'static SpaceResident,
        &'static TilePosition,
    ),
    Without<Player>,
>;

/// Query exposing every shopkeeper's stockpile by object id. The projection
/// uses this to materialize the wares list for `PlayerToShop` trade sessions.
#[cfg(feature = "server-sim")]
pub type ProjectionStockpileQuery<'w, 's> = Query<
    'w,
    's,
    (&'static OverworldObject, &'static Stockpile),
    (With<Shopkeeper>, Without<Player>),
>;

/// Per-peer memo that lets `compute_events_for_peer` skip the O(interest
/// window) per-tile floor diff on quiet frames: the scan's result can only
/// differ from last time if a grid mutated (`FloorMaps::revision`) or the
/// peer's window moved (their tile changed). Owned by the caller — a `Local`
/// in the embedded collect system, a peer field on the TCP flush path. Reset
/// to `default()` to force a full re-diff.
#[derive(Clone, Copy, Debug, Default)]
#[cfg(feature = "server-sim")]
pub struct FloorDiffCache {
    pub floor_maps_revision: Option<u64>,
    pub tile: Option<TilePosition>,
}

/// Diffs the authoritative ECS against a per-peer baseline, returning a
/// `Vec<GameEvent>` that, when folded into `previous`, produces the peer's
/// next `ClientGameState`. Passing `&ClientGameState::default()` as `previous`
/// yields a full bootstrap sequence for a newly connected client.
#[cfg(feature = "server-sim")]
pub fn compute_events_for_peer(
    local_player_id: PlayerId,
    previous: &ClientGameState,
    floor_diff_cache: &mut FloorDiffCache,
    player_query: &ProjectionPlayerQuery,
    object_query: &ProjectionObjectQuery,
    world_object_query: &ProjectionWorldObjectQuery,
    container_query: &ProjectionContainerQuery,
    stockpile_query: &ProjectionStockpileQuery,
    space_manager: &SpaceManager,
    floor_maps: &FloorMaps,
    world_clock: &WorldClock,
    active_trades: &ActiveTrades,
    parties: &crate::game::party::Parties,
    object_definitions: &OverworldObjectDefinitions,
) -> Vec<GameEvent> {
    let mut events = Vec::new();

    emit_world_time_events(previous, world_clock, &mut events);

    let (local, deferred_remote_candidates) = emit_player_events(
        local_player_id,
        previous,
        player_query,
        object_query,
        &mut events,
    );
    emit_remote_player_events(
        previous,
        local.space_id,
        local.tile,
        deferred_remote_candidates,
        &mut events,
    );
    // Before the positionless early-return: a dead (de-spatialized) viewer
    // must keep receiving roster updates behind the death overlay.
    emit_party_events(
        local_player_id,
        previous,
        parties,
        player_query,
        local.space_id,
        local.tile,
        &mut events,
    );

    let Some(local_space_id) = local.space_id else {
        return events;
    };
    // `tile` is set in the same branch as `space_id`; if we have one we have
    // the other. Unwrap here so all the downstream vicinity filters can read
    // a plain TilePosition.
    let local_tile = local
        .tile
        .expect("LocalPlayerContext::tile should be Some whenever space_id is Some (set together)");

    emit_floor_events(
        previous,
        floor_diff_cache,
        floor_maps,
        local_space_id,
        local_tile,
        &mut events,
    );
    emit_space_events(previous, space_manager, local_space_id, &mut events);
    emit_container_events(
        previous,
        container_query,
        local_space_id,
        local_tile,
        &mut events,
    );
    emit_world_object_events(
        local_player_id,
        previous,
        world_object_query,
        local_space_id,
        local_tile,
        local.entity,
        &local.revealed,
        &mut events,
    );
    emit_trade_events(
        local_player_id,
        previous,
        active_trades,
        player_query,
        stockpile_query,
        object_definitions,
        local.persuasion_ranks,
        &mut events,
    );

    events
}

/// Facts about the local player captured while iterating the player query in
/// [`emit_player_events`], consumed by the downstream per-domain emitters
/// (vicinity filters, awareness gating, trade pricing).
#[cfg(feature = "server-sim")]
struct LocalPlayerContext {
    space_id: Option<SpaceId>,
    tile: Option<TilePosition>,
    entity: Option<Entity>,
    persuasion_ranks: u8,
    /// NPC object ids the local player currently has a Perception "read" on
    /// (from `SenseReveals`); gates the awareness marker in
    /// [`emit_world_object_events`].
    revealed: std::collections::HashSet<u64>,
}

/// A remote player observed during the player loop, deferred so the
/// same-space + vicinity filter can use the local player's position
/// regardless of iteration order.
#[cfg(feature = "server-sim")]
type RemoteCandidate = (SpaceResident, TilePosition, ClientRemotePlayerState);

/// World-clock domain: emit `WorldTimeChanged` on epsilon move or heartbeat.
#[cfg(feature = "server-sim")]
fn emit_world_time_events(
    previous: &ClientGameState,
    world_clock: &WorldClock,
    events: &mut Vec<GameEvent>,
) {
    // World clock: emit on epsilon move OR heartbeat. Wraparound across the
    // 1.0 → 0.0 seam is handled by the rem_euclid distance below.
    let dt = (world_clock.time_of_day - previous.world_time + 1.0).rem_euclid(1.0);
    let dt_min = dt.min(1.0 - dt);
    if dt_min > WORLD_TIME_EPSILON
        || world_clock.seconds_since_emit >= WORLD_TIME_HEARTBEAT_SECS
        || previous.world_time == 0.0 && world_clock.time_of_day != 0.0
    {
        events.push(GameEvent::WorldTimeChanged {
            time_of_day: world_clock.time_of_day,
        });
    }
}

/// Player domain: diffs every local-player-facing slice of state (identity,
/// inventory, chat, position, vitals, buffs/effects, combat, progression,
/// discovery). Remote players encountered while iterating are deferred and
/// returned for [`emit_remote_player_events`].
#[cfg(feature = "server-sim")]
fn emit_player_events(
    local_player_id: PlayerId,
    previous: &ClientGameState,
    player_query: &ProjectionPlayerQuery,
    object_query: &ProjectionObjectQuery,
    events: &mut Vec<GameEvent>,
) -> (LocalPlayerContext, Vec<RemoteCandidate>) {
    let mut local_space_id: Option<SpaceId> = None;
    let mut local_tile_position: Option<TilePosition> = None;
    let mut local_persuasion_ranks: u8 = 0;
    let mut local_player_entity: Option<Entity> = None;
    // NPC object ids the local player currently has a Perception "read" on,
    // captured from their `SenseReveals` during the player loop and consumed
    // in the world-object loop to gate the awareness marker.
    let mut local_revealed: std::collections::HashSet<u64> = std::collections::HashSet::new();
    // Remote players are projected after the player loop ends so we know
    // local_space_id / local_tile_position regardless of iteration order.
    let mut deferred_remote_candidates: Vec<RemoteCandidate> = Vec::new();

    for (
        (player_entity, identity),
        inventory,
        chat_log,
        space_resident,
        tile_position,
        vital_stats,
        derived_stats,
        combat_target,
        player_object,
        facing,
        regen_buffs,
        (max_carry, current_carry, is_encumbered),
        experience,
        (class, stash, skill_sheet, appearance),
        (
            magic_effects,
            defense_stats,
            weapon_damage,
            attack_profile,
            discovered_tiles,
            is_sneaking,
            is_aware,
            is_auto_retaliate,
            sense_reveals,
            exertion,
        ),
    ) in player_query.iter()
    {
        let projected_facing = facing.copied().unwrap_or_default().0;
        let projected_vitals = ClientVitalStats {
            health: vital_stats.health,
            max_health: vital_stats.max_health,
            mana: vital_stats.mana,
            max_mana: vital_stats.max_mana,
        };

        if identity.id == local_player_id {
            local_player_entity = Some(player_entity);
            // Dead (de-spatialized) players have no position: leave the
            // context `None`, which freezes the client's floor/space/remote
            // view behind the death overlay (see `compute_events_for_peer`).
            if let (Some(space_resident), Some(tile_position)) = (space_resident, tile_position) {
                local_space_id = Some(space_resident.space_id);
                local_tile_position = Some(*tile_position);
            }
            if let Some(reveals) = sense_reveals {
                local_revealed = reveals.revealed.keys().copied().collect();
            }

            if previous.local_player_id != Some(local_player_id)
                || previous.local_player_object_id != Some(player_object.object_id)
            {
                events.push(GameEvent::LocalPlayerIdentified {
                    player_id: local_player_id,
                    object_id: player_object.object_id,
                });
            }

            diff_emit!(
                events,
                previous.inventory,
                *inventory,
                GameEvent::InventoryChanged {
                    inventory: inventory.clone(),
                },
            );

            diff_emit!(
                events,
                previous.chat_log_lines,
                chat_log.lines,
                GameEvent::ChatLogChanged {
                    lines: chat_log.lines.clone(),
                },
            );

            if let (Some(space_resident), Some(tile_position)) = (space_resident, tile_position) {
                let current_player_position =
                    SpacePosition::new(space_resident.space_id, *tile_position);
                if previous.player_position != Some(current_player_position)
                    || previous.player_facing != Some(projected_facing)
                {
                    events.push(GameEvent::PlayerPositionChanged {
                        position: current_player_position,
                        tile_position: *tile_position,
                        facing: projected_facing,
                    });
                }
            }

            diff_emit!(
                events,
                previous.player_vitals,
                Some(projected_vitals),
                GameEvent::PlayerVitalsChanged {
                    vitals: projected_vitals,
                },
            );

            diff_emit!(
                events,
                previous.player_storage_slots,
                derived_stats.storage_slots,
                GameEvent::PlayerStorageChanged {
                    storage_slots: derived_stats.storage_slots,
                },
            );

            // Carry weight: build a snapshot from the optional components.
            // Falls back to a default if either is missing (first frame
            // before refresh_derived_player_stats has run).
            let projected_carry = ClientCarryWeight {
                current_kg: current_carry.copied().unwrap_or_default().0,
                soft_cap_kg: max_carry.copied().unwrap_or_default().soft_cap,
                hard_cap_kg: max_carry.copied().unwrap_or_default().hard_cap,
                encumbered: is_encumbered,
            };
            let carry_changed = match previous.carry_weight {
                None => true,
                Some(prev) => {
                    (prev.current_kg - projected_carry.current_kg).abs() > 0.05
                        || (prev.soft_cap_kg - projected_carry.soft_cap_kg).abs() > 0.05
                        || (prev.hard_cap_kg - projected_carry.hard_cap_kg).abs() > 0.05
                        || prev.encumbered != projected_carry.encumbered
                }
            };
            if carry_changed {
                events.push(GameEvent::PlayerCarryWeightChanged {
                    carry: projected_carry,
                });
            }

            // Replicate active food/drink regen buff. We diff at integer-second
            // resolution on `remaining_seconds` so the HUD ticker animates
            // without spamming a wire event every frame.
            let projected_buff = regen_buffs.and_then(|buffs| {
                if buffs.is_active() {
                    Some(RegenBuffState {
                        multiplier: buffs.multiplier,
                        remaining_seconds: buffs.remaining_seconds,
                    })
                } else {
                    None
                }
            });
            let buff_changed = match (&previous.regen_buff, &projected_buff) {
                (None, None) => false,
                (Some(_), None) | (None, Some(_)) => true,
                (Some(a), Some(b)) => {
                    (a.multiplier - b.multiplier).abs() > f32::EPSILON
                        || a.remaining_seconds.floor() != b.remaining_seconds.floor()
                }
            };
            if buff_changed {
                events.push(GameEvent::PlayerRegenBuffChanged {
                    buff: projected_buff,
                });
            }

            // Replicate active magical effects (spell-driven buffs/debuffs on
            // the caster). Same integer-second debounce as RegenBuffs above;
            // the full vector is re-sent on any change.
            let projected_effects: Vec<ClientActiveEffect> = magic_effects
                .map(|effects| {
                    effects
                        .active
                        .iter()
                        .map(|effect| ClientActiveEffect {
                            kind: effect.kind,
                            magnitude: effect.magnitude,
                            remaining_seconds: effect.remaining_seconds,
                            secondary_magnitude: effect.secondary_magnitude,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let effects_changed = effects_diff(&previous.active_effects, &projected_effects);
            if effects_changed {
                events.push(GameEvent::PlayerEffectsChanged {
                    effects: projected_effects,
                });
            }

            diff_emit!(
                events,
                previous.sneaking,
                is_sneaking,
                GameEvent::PlayerSneakingChanged {
                    sneaking: is_sneaking,
                },
            );

            diff_emit!(
                events,
                previous.aware,
                is_aware,
                GameEvent::PlayerAwareChanged { aware: is_aware },
            );

            diff_emit!(
                events,
                previous.auto_retaliate,
                is_auto_retaliate,
                GameEvent::PlayerAutoRetaliateChanged {
                    auto_retaliate: is_auto_retaliate,
                },
            );

            // Exertion decays continuously, so diff at whole-point resolution
            // (and an epsilon on the cap) to avoid emitting an event every frame.
            let projected_exertion = exertion.map(|e| ClientExertion {
                current: e.current,
                max: e.max,
            });
            let exertion_changed = match (&previous.exertion, &projected_exertion) {
                (None, None) => false,
                (Some(_), None) | (None, Some(_)) => true,
                (Some(a), Some(b)) => {
                    a.current.round() != b.current.round() || (a.max - b.max).abs() > 0.5
                }
            };
            if exertion_changed {
                events.push(GameEvent::PlayerExertionChanged {
                    exertion: projected_exertion.unwrap_or_default(),
                });
            }

            let current_target_object_id = combat_target
                .and_then(|combat_target| object_query.get(combat_target.entity).ok())
                .map(|object| object.object_id);
            diff_emit!(
                events,
                previous.current_target_object_id,
                current_target_object_id,
                GameEvent::CombatTargetChanged {
                    target_object_id: current_target_object_id,
                },
            );

            let projected_experience: Option<ExperienceView> = experience.map(ExperienceView::from);
            if previous.experience != projected_experience {
                if let Some(view) = projected_experience {
                    events.push(GameEvent::PlayerExperienceChanged { experience: view });
                }
            }

            let projected_class = class.copied();
            if previous.class != projected_class {
                if let Some(c) = projected_class {
                    events.push(GameEvent::PlayerClassChanged { class: c });
                }
            }

            let projected_appearance = appearance.copied();
            if previous.appearance != projected_appearance {
                if let Some(a) = projected_appearance {
                    events.push(GameEvent::PlayerAppearanceChanged { appearance: a });
                }
            }

            let projected_attributes = derived_stats.attributes;
            diff_emit!(
                events,
                previous.attributes,
                Some(projected_attributes),
                GameEvent::PlayerAttributesChanged {
                    attributes: projected_attributes,
                },
            );

            // Combat stats. Server derives every displayed number so the UI
            // never has to mirror combat math — see CLAUDE.md "EmbeddedClient
            // Invariant". Formulas live in `crate::combat::formulas`.
            let projected_combat_stats = {
                let attrs = derived_stats.attributes;
                let level = experience.map(|e| e.level).unwrap_or(1);
                let (damage_min, damage_max) = crate::combat::formulas::weapon_damage_range(
                    &weapon_damage.0,
                    attrs,
                    level as i32,
                );
                // BAB track from the player's class so the displayed to-hit
                // matches what `resolve_battle_turn` actually rolls.
                let bab_track = projected_class
                    .map(|c| crate::player::classes::class_data(c).bab_track)
                    .unwrap_or(crate::player::classes::BabTrack::Full);
                let attack_bonus = crate::combat::formulas::attack_to_hit_bonus(
                    attack_profile.kind,
                    attrs,
                    bab_track,
                    level,
                    projected_class,
                );
                let dodge_dc = crate::combat::formulas::dodge_dc(
                    level,
                    attrs.agility,
                    defense_stats.dodge_bonus,
                );
                let has_shield = inventory
                    .equipment_item(crate::world::object_definitions::EquipmentSlot::Shield)
                    .is_some();
                let block_chance_pct = if has_shield {
                    crate::combat::formulas::effective_block_chance_pct(
                        defense_stats.block_chance,
                        attrs.agility,
                    )
                } else {
                    0
                };
                ClientCombatStats {
                    attack_kind: attack_profile.kind,
                    damage_type: attack_profile.damage_type,
                    damage_min,
                    damage_max,
                    attack_bonus,
                    dodge_dc,
                    armor: defense_stats.armor,
                    block: if has_shield { defense_stats.block } else { 0 },
                    block_chance_pct,
                    has_shield,
                }
            };
            diff_emit!(
                events,
                previous.combat_stats,
                Some(projected_combat_stats),
                GameEvent::PlayerCombatStatsChanged {
                    stats: projected_combat_stats,
                },
            );

            // Only the `recipes:known` slice of the stash is replicated.
            // Other stash entries (quest state, future features) stay on
            // the server.
            let projected_recipes = stash.map(|s| s.learned_recipes()).unwrap_or_default();
            diff_emit!(
                events,
                previous.learned_recipes,
                projected_recipes,
                GameEvent::LearnedRecipesChanged {
                    recipes: projected_recipes,
                },
            );

            // Replicate the per-character Log. Whole-state snapshot on any
            // change — fine until log payloads grow large enough that
            // per-entry deltas become worthwhile.
            let projected_log = stash
                .map(crate::log::LogState::from_stash)
                .unwrap_or_default();
            diff_emit!(
                events,
                previous.log_state,
                projected_log,
                GameEvent::LogStateChanged {
                    state: projected_log,
                },
            );

            // Replicate the skill sheet. Whole-snapshot diff against the
            // previous projection — small payload (10 u8s + a u32) so this
            // is fine even at autosave cadence.
            let projected_ranks = skill_sheet.map(|s| s.ranks).unwrap_or([0; 10]);
            let projected_points = skill_sheet.map(|s| s.available_points).unwrap_or(0);
            let projected_bumps = skill_sheet.map(|s| s.available_ability_bumps).unwrap_or(0);
            local_persuasion_ranks =
                projected_ranks[crate::player::skills::Skill::Persuasion.index()];
            if previous.skill_ranks != projected_ranks
                || previous.available_skill_points != projected_points
                || previous.available_ability_bumps != projected_bumps
            {
                events.push(GameEvent::SkillSheetChanged {
                    ranks: projected_ranks,
                    available_points: projected_points,
                    available_ability_bumps: projected_bumps,
                });
            }

            // Discovered tiles delta. Group new tiles per space and emit one
            // event per space. `previous` is empty on bootstrap, so the first
            // tick after login naturally ships the full saved set as deltas.
            if let Some(authoritative) = discovered_tiles {
                for (space_id, auth_set) in authoritative.by_space.iter() {
                    let empty = std::collections::HashSet::new();
                    let prev_set = previous.discovered_tiles.get(space_id).unwrap_or(&empty);
                    let new_tiles: Vec<(i32, i32)> = auth_set
                        .iter()
                        .filter(|t| !prev_set.contains(t))
                        .copied()
                        .collect();
                    if !new_tiles.is_empty() {
                        events.push(GameEvent::TilesDiscovered {
                            space_id: *space_id,
                            tiles: new_tiles,
                        });
                    }
                }
            }
        } else {
            // Dead (de-spatialized) remote players have no position and are
            // never candidates; the removal loop in
            // `emit_remote_player_events` then drops them from this peer's
            // view. They reappear automatically once respawn re-inserts the
            // spatial components.
            let (Some(space_resident), Some(tile_position)) = (space_resident, tile_position)
            else {
                continue;
            };
            // Defer projection until after the player loop so we know
            // local_space_id / local_tile_position before applying the
            // same-space + vicinity filter, regardless of iteration order.
            let position = SpacePosition::new(space_resident.space_id, *tile_position);
            deferred_remote_candidates.push((
                *space_resident,
                *tile_position,
                ClientRemotePlayerState {
                    player_id: identity.id,
                    object_id: player_object.object_id,
                    position,
                    tile_position: *tile_position,
                    vitals: projected_vitals,
                    facing: projected_facing,
                    class: class.copied().unwrap_or_default(),
                    appearance: appearance.copied().unwrap_or_default(),
                },
            ));
        }
    }

    (
        LocalPlayerContext {
            space_id: local_space_id,
            tile: local_tile_position,
            entity: local_player_entity,
            persuasion_ranks: local_persuasion_ranks,
            revealed: local_revealed,
        },
        deferred_remote_candidates,
    )
}

/// Remote-player domain: same-space + vicinity filter over the deferred
/// candidates, upserting changed entries and removing baseline entries that
/// fell out of view.
#[cfg(feature = "server-sim")]
fn emit_remote_player_events(
    previous: &ClientGameState,
    local_space_id: Option<SpaceId>,
    local_tile_position: Option<TilePosition>,
    mut deferred_remote_candidates: Vec<RemoteCandidate>,
    events: &mut Vec<GameEvent>,
) {
    let mut seen_remote_player_ids: std::collections::HashSet<PlayerId> =
        std::collections::HashSet::new();

    let (Some(local_space), Some(local_tile)) = (local_space_id, local_tile_position) else {
        // The local player has no position — either pre-spawn bootstrap
        // (baseline is empty, nothing to remove) or dead behind the death
        // overlay. Emitting removals here would strip every remote sprite
        // from the frozen scene, so hold the view instead.
        return;
    };

    for (resident, tile, projected) in deferred_remote_candidates.drain(..) {
        if resident.space_id != local_space || !in_interest_radius(local_tile, tile) {
            continue;
        }
        seen_remote_player_ids.insert(projected.player_id);
        diff_emit!(
            events,
            previous.remote_players.get(&projected.player_id),
            Some(&projected),
            GameEvent::RemotePlayerUpserted { player: projected },
        );
    }

    for previous_id in previous.remote_players.keys() {
        if !seen_remote_player_ids.contains(previous_id) {
            events.push(GameEvent::RemotePlayerRemoved {
                player_id: *previous_id,
            });
        }
    }
}

/// Floor domain: full-grid `FloorMapReplaced` at bootstrap / on resize, plus
/// vicinity-filtered per-tile `FloorTileSet` deltas, memoized through
/// `floor_diff_cache` on quiet frames.
#[cfg(feature = "server-sim")]
fn emit_floor_events(
    previous: &ClientGameState,
    floor_diff_cache: &mut FloorDiffCache,
    floor_maps: &FloorMaps,
    local_space_id: SpaceId,
    local_tile: TilePosition,
    events: &mut Vec<GameEvent>,
) {
    // The per-tile window diff below is O((2R+1)²) per z-level — ~3.7k tile
    // compares per floor per peer. Its outcome can only change when a grid
    // mutated or this peer's window moved, so skip it on quiet frames. The
    // bootstrap / resize arms (full `FloorMapReplaced`) stay active: they key
    // off `previous.floor_maps` presence, which the memo does not cover.
    let window_scan_needed = floor_diff_cache.floor_maps_revision != Some(floor_maps.revision())
        || floor_diff_cache.tile != Some(local_tile);
    floor_diff_cache.floor_maps_revision = Some(floor_maps.revision());
    floor_diff_cache.tile = Some(local_tile);

    // Push every floor map *before* CurrentSpaceChanged so the renderer sees
    // each (space, z) grid populated by the time the space switch triggers a
    // rebuild on the next frame. Replicates every z that exists in `FloorMaps`
    // for the local space — Tibia-style multi-floor rendering needs upper
    // floors to reach the client.
    for (space_id_iter, z, server_floor_map) in floor_maps.iter() {
        if space_id_iter != local_space_id {
            continue;
        }
        match previous.floor_maps.get(&(local_space_id, z)) {
            None => {
                events.push(GameEvent::FloorMapReplaced {
                    space_id: local_space_id,
                    z,
                    width: server_floor_map.width,
                    height: server_floor_map.height,
                    tiles: server_floor_map.tiles.clone(),
                });
            }
            Some(prev)
                if prev.width != server_floor_map.width
                    || prev.height != server_floor_map.height =>
            {
                events.push(GameEvent::FloorMapReplaced {
                    space_id: local_space_id,
                    z,
                    width: server_floor_map.width,
                    height: server_floor_map.height,
                    tiles: server_floor_map.tiles.clone(),
                });
            }
            Some(prev) => {
                // Per-tile deltas are vicinity-filtered: only emit changes
                // within INTEREST_RADIUS on the local player's floor. The
                // FloorMapReplaced arms above ship the full grid at bootstrap
                // / on resize, so distant tiles already populated on the
                // client will be repaired on the first tick the player walks
                // back into range (their per-peer baseline still has the old
                // tile, so prev != current and the delta fires).
                if !window_scan_needed || local_tile.z != z {
                    continue;
                }
                let r = INTEREST_RADIUS.ceil() as i32;
                let x_min = (local_tile.x - r).max(0);
                let x_max = (local_tile.x + r + 1).min(server_floor_map.width);
                let y_min = (local_tile.y - r).max(0);
                let y_max = (local_tile.y + r + 1).min(server_floor_map.height);
                for y in y_min..y_max {
                    for x in x_min..x_max {
                        let idx = (y * server_floor_map.width + x) as usize;
                        if prev.tiles[idx] != server_floor_map.tiles[idx]
                            && in_interest_radius(local_tile, TilePosition::new(x, y, z))
                        {
                            events.push(GameEvent::FloorTileSet {
                                space_id: local_space_id,
                                z,
                                x,
                                y,
                                floor_type: server_floor_map.tiles[idx].clone(),
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Space domain: `CurrentSpaceChanged` when the local player's space (or its
/// metadata) differs from the baseline.
#[cfg(feature = "server-sim")]
fn emit_space_events(
    previous: &ClientGameState,
    space_manager: &SpaceManager,
    local_space_id: SpaceId,
    events: &mut Vec<GameEvent>,
) {
    if let Some(runtime_space) = space_manager.get(local_space_id) {
        let current_space = ClientSpaceState {
            space_id: runtime_space.id,
            authored_id: runtime_space.authored_id.clone(),
            width: runtime_space.width,
            height: runtime_space.height,
            fill_floor_type: runtime_space.fill_floor_type.clone(),
            lighting: runtime_space.lighting.clone(),
        };
        diff_emit!(
            events,
            previous.current_space.as_ref(),
            Some(&current_space),
            GameEvent::CurrentSpaceChanged {
                space: current_space,
            },
        );
    }
}

/// Container domain: upserts changed in-range container slot lists and
/// removes baseline entries that fell out of view.
#[cfg(feature = "server-sim")]
fn emit_container_events(
    previous: &ClientGameState,
    container_query: &ProjectionContainerQuery,
    local_space_id: SpaceId,
    local_tile: TilePosition,
    events: &mut Vec<GameEvent>,
) {
    let mut current_container_ids = std::collections::HashSet::new();
    for (container, object, resident, tile_position) in container_query.iter() {
        if resident.space_id != local_space_id {
            continue;
        }
        if !in_interest_radius(local_tile, *tile_position) {
            continue;
        }
        current_container_ids.insert(object.object_id);
        let current_slots = &container.slots;
        diff_emit!(
            events,
            previous.container_slots.get(&object.object_id),
            Some(current_slots),
            GameEvent::ContainerChanged {
                object_id: object.object_id,
                slots: current_slots.clone(),
            },
        );
    }

    for stale_object_id in previous.container_slots.keys() {
        if !current_container_ids.contains(stale_object_id) {
            events.push(GameEvent::ContainerRemoved {
                object_id: *stale_object_id,
            });
        }
    }
}

/// World-object domain: upserts changed in-range world objects (with the
/// hidden-object and awareness gating) and removes baseline entries that
/// fell out of view.
#[cfg(feature = "server-sim")]
fn emit_world_object_events(
    local_player_id: PlayerId,
    previous: &ClientGameState,
    world_object_query: &ProjectionWorldObjectQuery,
    local_space_id: SpaceId,
    local_tile: TilePosition,
    local_player_entity: Option<Entity>,
    local_revealed: &std::collections::HashSet<u64>,
    events: &mut Vec<GameEvent>,
) {
    let mut current_world_object_ids = std::collections::HashSet::new();
    for (
        space_resident,
        tile_position,
        object,
        vitals,
        has_container,
        has_npc,
        has_movable,
        has_rotatable,
        qty,
        has_dialog,
        facing,
        state,
        has_shopkeeper,
        hidden,
        (has_hostile, npc_combat_target, ai_state, npc_faction, npc_tags, npc_guilt, npc_judge),
    ) in world_object_query.iter()
    {
        if space_resident.space_id != local_space_id {
            continue;
        }
        // Hidden trait: filter objects the local player hasn't spotted. The
        // diff loop below handles WorldObjectRemoved for objects that
        // previously appeared (e.g. across a transient detection state).
        if let Some(h) = hidden {
            if !h.is_detected_by(local_player_id) {
                continue;
            }
        }
        if !in_interest_radius(local_tile, *tile_position) {
            continue;
        }
        current_world_object_ids.insert(object.object_id);
        let is_targeting_local_player = match (npc_combat_target, local_player_entity) {
            (Some(target), Some(local_entity)) => target.entity == local_entity,
            _ => false,
        };
        // "Hostile" is per-viewer: does this NPC's hostility model make it an
        // aggressor toward the player side? A town guard (PlayerSide, hostile
        // only toward monster tags) has combat AI but renders peaceful; a
        // goblin (MonsterSide) is red as ever. The MonsterSide fallback keeps
        // legacy faction-less hostiles red.
        let is_hostile_to_viewer = has_hostile
            && crate::npc::hostility::is_hostile_toward(
                crate::npc::hostility::Aggressor::new(
                    npc_faction
                        .copied()
                        .unwrap_or(crate::npc::components::Faction::MonsterSide),
                    npc_tags.map(|t| t.hostile_towards).unwrap_or_default(),
                    npc_guilt,
                ),
                // The viewer themself is the subject, so a guard the local
                // player has wronged renders red to *them* while staying
                // peaceful for the innocent standing beside them.
                crate::npc::hostility::Subject::new(
                    crate::npc::components::Faction::PlayerSide,
                    crate::npc::hostility::TagMask::PLAYER,
                    Some(local_player_id),
                ),
            );
        // Awareness marker: only for hostile NPCs the local player has read
        // (passed a Perception check, tracked in `SenseReveals`). Alerted if it
        // targets us, Searching if it's in Alert, else Unaware.
        let awareness = if is_hostile_to_viewer && local_revealed.contains(&object.object_id) {
            use crate::npc::components::AiState;
            let level = if is_targeting_local_player {
                NpcAwareness::Alerted
            } else if matches!(
                ai_state,
                // A fleeing NPC has no CombatTarget but absolutely knows
                // something is out there — "Searching" reads truer than
                // "Unaware" while it runs.
                Some(AiState::Alert { .. }) | Some(AiState::Flee { .. })
            ) {
                NpcAwareness::Searching
            } else {
                NpcAwareness::Unaware
            };
            Some(level)
        } else {
            None
        };
        let projected_vitals = vitals.map(|vitals| ClientVitalStats {
            health: vitals.health,
            max_health: vitals.max_health,
            mana: vitals.mana,
            max_mana: vitals.max_mana,
        });

        // Compare against the baseline *before* materializing the projected
        // struct: building `ClientWorldObjectState` clones two Strings
        // (definition_id, state), and doing that for every in-range object on
        // every frame per peer dominated idle projection cost. The exhaustive
        // destructure (no `..`) means adding a field to the struct is a
        // compile error here — keep it in sync with the constructor below.
        let unchanged = previous.world_objects.get(&object.object_id).is_some_and(
            |ClientWorldObjectState {
                 object_id: _,
                 definition_id: prev_definition_id,
                 position: prev_position,
                 tile_position: prev_tile_position,
                 vitals: prev_vitals,
                 is_container: prev_is_container,
                 is_npc: prev_is_npc,
                 is_movable: prev_is_movable,
                 is_rotatable: prev_is_rotatable,
                 quantity: prev_quantity,
                 has_dialog: prev_has_dialog,
                 facing: prev_facing,
                 state: prev_state,
                 is_shopkeeper: prev_is_shopkeeper,
                 is_judge: prev_is_judge,
                 is_hidden: prev_is_hidden,
                 is_hostile: prev_is_hostile,
                 is_targeting_local_player: prev_is_targeting,
                 awareness: prev_awareness,
                 placement_seq: prev_placement_seq,
             }| {
                *prev_definition_id == object.definition_id
                    && *prev_position == SpacePosition::new(space_resident.space_id, *tile_position)
                    && prev_tile_position == tile_position
                    && *prev_vitals == projected_vitals
                    && *prev_is_container == has_container
                    && *prev_is_npc == has_npc
                    && *prev_is_movable == has_movable
                    && *prev_is_rotatable == has_rotatable
                    && *prev_quantity == qty.map(|q| q.0).unwrap_or(1)
                    && *prev_has_dialog == has_dialog
                    && *prev_facing == facing.copied().unwrap_or_default().0
                    && prev_state.as_deref() == state.map(|s| s.0.as_str())
                    && *prev_is_shopkeeper == has_shopkeeper
                    && *prev_is_judge == npc_judge.is_some()
                    && *prev_is_hidden == hidden.is_some()
                    && *prev_is_hostile == is_hostile_to_viewer
                    && *prev_is_targeting == is_targeting_local_player
                    && *prev_awareness == awareness
                    && *prev_placement_seq == object.placement_seq
            },
        );
        if unchanged {
            continue;
        }

        let projected_object = ClientWorldObjectState {
            object_id: object.object_id,
            definition_id: object.definition_id.clone(),
            position: SpacePosition::new(space_resident.space_id, *tile_position),
            tile_position: *tile_position,
            vitals: projected_vitals,
            is_container: has_container,
            is_npc: has_npc,
            is_movable: has_movable,
            is_rotatable: has_rotatable,
            quantity: qty.map(|q| q.0).unwrap_or(1),
            has_dialog,
            facing: facing.copied().unwrap_or_default().0,
            state: state.map(|s| s.0.clone()),
            is_shopkeeper: has_shopkeeper,
            is_judge: npc_judge.is_some(),
            is_hidden: hidden.is_some(),
            is_hostile: is_hostile_to_viewer,
            is_targeting_local_player,
            awareness,
            placement_seq: object.placement_seq,
        };

        events.push(GameEvent::WorldObjectUpserted {
            object: projected_object,
        });
    }

    for stale_object_id in previous.world_objects.keys() {
        if !current_world_object_ids.contains(stale_object_id) {
            events.push(GameEvent::WorldObjectRemoved {
                object_id: *stale_object_id,
            });
        }
    }
}

/// Party domain: projects the viewer's party roster and diffs the whole
/// `Option<ClientPartyView>` against the baseline.
///
/// Unlike `remote_players`, rows are never vicinity-pruned — a member across
/// the map (or de-spatialized by death) stays listed with `in_range: false`.
/// Vitals are rounded to whole points so passive regen doesn't re-emit the
/// roster every tick.
#[cfg(feature = "server-sim")]
fn emit_party_events(
    local_player_id: PlayerId,
    previous: &ClientGameState,
    parties: &crate::game::party::Parties,
    player_query: &ProjectionPlayerQuery,
    viewer_space: Option<SpaceId>,
    viewer_tile: Option<TilePosition>,
    events: &mut Vec<GameEvent>,
) {
    use crate::game::party::{
        share_percentages, ClientPartyView, PartyMemberView, PARTY_SHARE_RADIUS_TILES,
    };
    use crate::world::components::tile_distance_3d;

    let projected_party = parties.party_for(local_player_id).map(|party| {
        let mut members: Vec<PartyMemberView> = Vec::with_capacity(party.members.len());
        for member_id in &party.members {
            // Members are evicted by `cleanup_invalid_parties` (ordered before
            // the flush) the tick their entity disappears, so a miss here is a
            // sub-tick window; skip the row rather than inventing a ghost.
            let Some((
                (_, identity),
                _,
                _,
                space_resident,
                tile_position,
                vital_stats,
                ..,
                player_object,
                _,
                _,
                _,
                experience,
                (class, ..),
                _,
            )) = player_query
                .iter()
                .find(|((_, identity), ..)| identity.id == *member_id)
            else {
                continue;
            };

            let space_id = space_resident.map(|resident| resident.space_id);
            let tile = tile_position.copied();
            let in_range = *member_id == local_player_id
                || match (viewer_space, viewer_tile, space_id, tile) {
                    (Some(vs), Some(vt), Some(ms), Some(mt)) => {
                        vs == ms && tile_distance_3d(vt, mt) <= PARTY_SHARE_RADIUS_TILES
                    }
                    _ => false,
                };

            members.push(PartyMemberView {
                player_id: *member_id,
                display_name: identity.display_name.clone(),
                level: experience.map(|e| e.level).unwrap_or(1),
                class: class.copied().unwrap_or_default(),
                object_id: Some(player_object.object_id),
                vitals: ClientVitalStats {
                    health: vital_stats.health.round(),
                    max_health: vital_stats.max_health.round(),
                    mana: vital_stats.mana.round(),
                    max_mana: vital_stats.max_mana.round(),
                },
                space_id,
                tile,
                online: true,
                in_range,
                is_leader: *member_id == party.leader,
                share_pct: 0,
            });
        }

        // Share percentages are computed over the in-range subset only —
        // out-of-range members earn nothing from a kill next to the viewer.
        let eligible: Vec<(PlayerId, u32)> = members
            .iter()
            .filter(|member| member.in_range)
            .map(|member| (member.player_id, member.level))
            .collect();
        let percentages = share_percentages(&eligible);
        for (index, (player_id, _)) in eligible.iter().enumerate() {
            if let Some(member) = members
                .iter_mut()
                .find(|member| member.player_id == *player_id)
            {
                member.share_pct = percentages[index];
            }
        }

        ClientPartyView {
            party_id: party.party_id,
            leader: party.leader,
            members,
            focus_target: party.focus_target,
        }
    });

    diff_emit!(
        events,
        previous.party,
        projected_party,
        GameEvent::PartyStateChanged {
            party: projected_party,
        },
    );
}

/// Trade domain: projects the local player's active trade session (partner
/// name, shop wares with persuasion-adjusted prices) and diffs it against
/// the baseline.
#[cfg(feature = "server-sim")]
fn emit_trade_events(
    local_player_id: PlayerId,
    previous: &ClientGameState,
    active_trades: &ActiveTrades,
    player_query: &ProjectionPlayerQuery,
    stockpile_query: &ProjectionStockpileQuery,
    object_definitions: &OverworldObjectDefinitions,
    local_persuasion_ranks: u8,
    events: &mut Vec<GameEvent>,
) {
    // Trade projection: find the local player's active trade (if any) and
    // diff its `ClientTradeView` against the previous baseline. Partner name
    // is built from the partner's PlayerId / NPC display name; for shop
    // sessions we also project the wares list so the panel can render the
    // Browse Wares subpanel.
    let projected_trade =
        active_trades
            .find_for_player(local_player_id)
            .and_then(|(session_id, _side)| {
                active_trades.sessions.get(&session_id).and_then(|session| {
                    match session.participants {
                        TradeParticipants::PlayerToPlayer { a, b } => {
                            let partner_id = if a == local_player_id { b } else { a };
                            let partner_name = player_query
                                .iter()
                                .find(|((_, identity), ..)| identity.id == partner_id)
                                .map(|((_, identity), ..)| identity.display_name.clone())
                                .unwrap_or_else(|| format!("Player {}", partner_id.0));
                            session.project_for(
                                local_player_id,
                                partner_name,
                                TradePartnerKind::Player,
                                None,
                                // Players don't buy from each other for coin
                                // by the item — there's nothing to credit.
                                0,
                            )
                        }
                        TradeParticipants::PlayerToShop { shop_object_id, .. } => {
                            let stockpile_entry = stockpile_query
                                .iter()
                                .find(|(object, _)| object.object_id == shop_object_id);
                            let (partner_name, wares) = match stockpile_entry {
                                Some((object, stockpile)) => {
                                    let partner_name = object_definitions
                                        .get(&object.definition_id)
                                        .map(|def| def.name.clone())
                                        .unwrap_or_else(|| object.definition_id.clone());
                                    let wares: Vec<WareView> = stockpile
                                        .wares
                                        .iter()
                                        .map(|entry| {
                                            let display_name = object_definitions
                                                .get(&entry.type_id)
                                                .map(|def| def.name.clone())
                                                .unwrap_or_else(|| entry.type_id.clone());
                                            let stock_remaining = match entry.stock {
                                                StockMode::Infinite => None,
                                                StockMode::Finite(n) => Some(n),
                                            };
                                            let modified_price =
                                                crate::game::trade::vendor_price_for(
                                                    local_persuasion_ranks,
                                                    entry.price_copper,
                                                    crate::game::trade::TradeSide::PlayerBuys,
                                                );
                                            WareView {
                                                type_id: entry.type_id.clone(),
                                                display_name,
                                                price_copper: modified_price,
                                                stock_remaining,
                                                persuasion_modifier_pct:
                                                    crate::game::trade::persuasion_modifier_pct(
                                                        local_persuasion_ranks,
                                                        crate::game::trade::TradeSide::PlayerBuys,
                                                    ),
                                            }
                                        })
                                        .collect();
                                    (partner_name, Some(wares))
                                }
                                None => ("Shopkeeper".to_owned(), None),
                            };
                            // Preview what the merchant will pay for what we
                            // have already put in our column. Same function
                            // the commit path uses, so the preview cannot
                            // disagree with the payout.
                            let sale_credit = session
                                .offers_a
                                .iter()
                                .map(|entry| {
                                    crate::game::trade::offer_credit_copper(
                                        entry,
                                        object_definitions,
                                        local_persuasion_ranks,
                                    )
                                })
                                .fold(0u32, |acc, v| acc.saturating_add(v));
                            session.project_for(
                                local_player_id,
                                partner_name,
                                TradePartnerKind::Shopkeeper,
                                wares,
                                sale_credit,
                            )
                        }
                    }
                })
            });
    diff_emit!(
        events,
        previous.current_trade,
        projected_trade,
        GameEvent::TradeStateChanged {
            state: projected_trade,
        },
    );
}

pub fn apply_game_events_to_client_state(
    mut client_state: ResMut<ClientGameState>,
    mut pending_game_events: ResMut<PendingGameEvents>,
    mut revisions: ResMut<ClientStateRevisions>,
) {
    let _t = crate::diagnostics::SystemTimer::new("apply_game_events_to_client_state", 1.0);
    let events = std::mem::take(&mut pending_game_events.events);
    for event in events {
        log_client_game_event(&client_state, &event);
        // Bump per-domain counters so presentation systems that read a large
        // slice of state can gate on a cheap `u64` instead of the monolithic
        // `ClientGameState::is_changed()`, which is dirtied by *any* event.
        match &event {
            GameEvent::WorldObjectUpserted { .. } | GameEvent::WorldObjectRemoved { .. } => {
                revisions.world_objects = revisions.world_objects.wrapping_add(1);
            }
            GameEvent::RemotePlayerUpserted { .. } | GameEvent::RemotePlayerRemoved { .. } => {
                revisions.remote_players = revisions.remote_players.wrapping_add(1);
            }
            GameEvent::FloorMapReplaced { .. } | GameEvent::FloorTileSet { .. } => {
                revisions.map_tiles = revisions.map_tiles.wrapping_add(1);
                revisions.floor_maps = revisions.floor_maps.wrapping_add(1);
            }
            GameEvent::DiscoveredTilesReplaced { .. } | GameEvent::TilesDiscovered { .. } => {
                revisions.map_tiles = revisions.map_tiles.wrapping_add(1);
                revisions.discovered = revisions.discovered.wrapping_add(1);
            }
            GameEvent::LogStateChanged { .. } => {
                revisions.log = revisions.log.wrapping_add(1);
            }
            GameEvent::InventoryChanged { .. }
            | GameEvent::ContainerChanged { .. }
            | GameEvent::ContainerRemoved { .. }
            | GameEvent::PlayerStorageChanged { .. } => {
                revisions.inventory = revisions.inventory.wrapping_add(1);
            }
            GameEvent::PartyStateChanged { .. } => {
                revisions.party = revisions.party.wrapping_add(1);
            }
            _ => {}
        }
        apply_event_to_state(&mut client_state, event);
    }
}

/// Folds a single `GameEvent` into a `ClientGameState` — used both by
/// `apply_game_events_to_client_state` (on the client) and the per-peer
/// baseline-advance on the server.
pub fn apply_event_to_state(state: &mut ClientGameState, event: GameEvent) {
    match event {
        GameEvent::LocalPlayerIdentified {
            player_id,
            object_id,
        } => {
            state.local_player_id = Some(player_id);
            state.local_player_object_id = Some(object_id);
        }
        GameEvent::InventoryChanged { inventory } => {
            state.inventory = inventory;
        }
        GameEvent::ChatLogChanged { lines } => {
            state.chat_log_lines = lines;
        }
        GameEvent::PlayerPositionChanged {
            position,
            tile_position,
            facing,
        } => {
            state.player_position = Some(position);
            state.player_tile_position = Some(tile_position);
            state.player_facing = Some(facing);
        }
        GameEvent::CurrentSpaceChanged { space } => {
            state.current_space = Some(space);
        }
        GameEvent::PlayerVitalsChanged { vitals } => {
            state.player_vitals = Some(vitals);
        }
        GameEvent::PlayerRegenBuffChanged { buff } => {
            state.regen_buff = buff;
        }
        GameEvent::PlayerEffectsChanged { effects } => {
            state.active_effects = effects;
        }
        GameEvent::PlayerSneakingChanged { sneaking } => {
            state.sneaking = sneaking;
        }
        GameEvent::PlayerAwareChanged { aware } => {
            state.aware = aware;
        }
        GameEvent::PlayerAutoRetaliateChanged { auto_retaliate } => {
            state.auto_retaliate = auto_retaliate;
        }
        GameEvent::PlayerExertionChanged { exertion } => {
            state.exertion = Some(exertion);
        }
        GameEvent::PlayerStorageChanged { storage_slots } => {
            state.player_storage_slots = storage_slots;
        }
        GameEvent::PlayerCarryWeightChanged { carry } => {
            state.carry_weight = Some(carry);
        }
        GameEvent::CombatTargetChanged { target_object_id } => {
            state.current_target_object_id = target_object_id;
        }
        GameEvent::ContainerChanged { object_id, slots } => {
            state.container_slots.insert(object_id, slots);
        }
        GameEvent::ContainerRemoved { object_id } => {
            state.container_slots.remove(&object_id);
        }
        GameEvent::WorldObjectUpserted { object } => {
            state.world_objects.insert(object.object_id, object);
        }
        GameEvent::WorldObjectRemoved { object_id } => {
            state.world_objects.remove(&object_id);
        }
        GameEvent::RemotePlayerUpserted { player } => {
            state.remote_players.insert(player.player_id, player);
        }
        GameEvent::RemotePlayerRemoved { player_id } => {
            state.remote_players.remove(&player_id);
        }
        GameEvent::FloorMapReplaced {
            space_id,
            z,
            width,
            height,
            tiles,
        } => {
            let map = crate::world::floor_map::FloorMap {
                width,
                height,
                tiles,
            };
            state.floor_maps.insert((space_id, z), map);
        }
        GameEvent::FloorTileSet {
            space_id,
            z,
            x,
            y,
            floor_type,
        } => {
            if let Some(map) = state.floor_maps.get_mut(&(space_id, z)) {
                let _ = map.set(x, y, floor_type);
            }
        }
        GameEvent::WorldTimeChanged { time_of_day } => {
            state.world_time = time_of_day;
        }
        GameEvent::PlayerExperienceChanged { experience } => {
            state.experience = Some(experience);
        }
        GameEvent::PlayerClassChanged { class } => {
            state.class = Some(class);
        }
        GameEvent::PlayerAppearanceChanged { appearance } => {
            state.appearance = Some(appearance);
        }
        GameEvent::PlayerAttributesChanged { attributes } => {
            state.attributes = Some(attributes);
        }
        GameEvent::PlayerCombatStatsChanged { stats } => {
            state.combat_stats = Some(stats);
        }
        GameEvent::TradeStateChanged { state: new_state } => {
            state.current_trade = new_state;
        }
        GameEvent::PartyStateChanged { party } => {
            state.party = party;
        }
        GameEvent::LearnedRecipesChanged { recipes } => {
            state.learned_recipes = recipes;
        }
        GameEvent::LogStateChanged { state: log_state } => {
            state.log_state = log_state;
        }
        GameEvent::SkillSheetChanged {
            ranks,
            available_points,
            available_ability_bumps,
        } => {
            state.skill_ranks = ranks;
            state.available_skill_points = available_points;
            state.available_ability_bumps = available_ability_bumps;
        }
        GameEvent::DiscoveredTilesReplaced { tiles } => {
            state.discovered_tiles.clear();
            for (space_id, list) in tiles {
                state
                    .discovered_tiles
                    .insert(space_id, list.into_iter().collect());
            }
        }
        GameEvent::TilesDiscovered { space_id, tiles } => {
            let set = state.discovered_tiles.entry(space_id).or_default();
            for tile in tiles {
                set.insert(tile);
            }
        }
    }
}

fn log_client_game_event(client_state: &ClientGameState, event: &GameEvent) {
    match event {
        GameEvent::LocalPlayerIdentified {
            player_id,
            object_id,
        } => info!(
            "client local player identified: {:?} object {} (was {:?}/{:?})",
            player_id,
            object_id,
            client_state.local_player_id,
            client_state.local_player_object_id,
        ),
        GameEvent::InventoryChanged { inventory } => info!(
            "client inventory updated: {} backpack slots used, {} equipped slots occupied",
            inventory.backpack_slots.iter().flatten().count(),
            inventory
                .equipment_slots
                .iter()
                .filter(|(_, item)| item.is_some())
                .count(),
        ),
        GameEvent::ChatLogChanged { lines } => {
            let previous_count = client_state.chat_log_lines.len();
            let new_count = lines.len();
            if new_count > previous_count {
                if let Some(last_line) = lines.last() {
                    info!("client chat log appended: {last_line}");
                }
            } else {
                debug!(
                    "client chat log replaced: {} -> {} lines",
                    previous_count, new_count
                );
            }
        }
        // Fires on every step — keep off the default (info) log level.
        GameEvent::PlayerPositionChanged { position, .. } => debug!(
            "client player position updated: {:?} -> space {} at ({}, {})",
            client_state.player_position,
            position.space_id.0,
            position.tile_position.x,
            position.tile_position.y
        ),
        GameEvent::CurrentSpaceChanged { space } => info!(
            "client current space updated: {:?} -> {} ({})",
            client_state
                .current_space
                .as_ref()
                .map(|current| current.space_id.0),
            space.space_id.0,
            space.authored_id
        ),
        // Fires on every integer HP/mana tick (regen, combat) — debug only.
        GameEvent::PlayerVitalsChanged { vitals } => debug!(
            "client player vitals updated: hp {:.1}/{:.1} -> {:.1}/{:.1}, mana {:.1}/{:.1} -> {:.1}/{:.1}",
            client_state.player_vitals.map(|current| current.health).unwrap_or_default(),
            client_state.player_vitals.map(|current| current.max_health).unwrap_or_default(),
            vitals.health,
            vitals.max_health,
            client_state.player_vitals.map(|current| current.mana).unwrap_or_default(),
            client_state.player_vitals.map(|current| current.max_mana).unwrap_or_default(),
            vitals.mana,
            vitals.max_mana
        ),
        GameEvent::PlayerRegenBuffChanged { buff } => debug!(
            "client regen buff updated: {:?} -> {:?}",
            client_state.regen_buff, buff
        ),
        GameEvent::PlayerEffectsChanged { effects } => debug!(
            "client magic effects updated: {:?} -> {:?}",
            client_state.active_effects, effects
        ),
        GameEvent::PlayerSneakingChanged { sneaking } => debug!(
            "client sneaking updated: {} -> {}",
            client_state.sneaking, sneaking
        ),
        GameEvent::PlayerAwareChanged { aware } => debug!(
            "client aware updated: {} -> {}",
            client_state.aware, aware
        ),
        GameEvent::PlayerAutoRetaliateChanged { auto_retaliate } => debug!(
            "client auto-retaliate updated: {} -> {}",
            client_state.auto_retaliate, auto_retaliate
        ),
        GameEvent::PlayerExertionChanged { exertion } => debug!(
            "client exertion updated: {:?} -> {:.0}/{:.0}",
            client_state.exertion.map(|e| e.current),
            exertion.current,
            exertion.max
        ),
        GameEvent::PlayerStorageChanged { storage_slots } => info!(
            "client player storage updated: {} -> {}",
            client_state.player_storage_slots, storage_slots
        ),
        GameEvent::PlayerCarryWeightChanged { carry } => debug!(
            "client carry weight: {:.1}/{:.1} kg, encumbered={}",
            carry.current_kg, carry.soft_cap_kg, carry.encumbered
        ),
        GameEvent::CombatTargetChanged { target_object_id } => info!(
            "client combat target updated: {:?} -> {:?}",
            client_state.current_target_object_id, target_object_id
        ),
        GameEvent::ContainerChanged { object_id, slots } => debug!(
            "client container {} updated: {} slots",
            object_id,
            slots.len()
        ),
        GameEvent::ContainerRemoved { object_id } => {
            debug!("client container {} removed from projection", object_id)
        }
        GameEvent::WorldObjectUpserted { object } => debug!(
            "client projected object upserted: {} ({}) at ({}, {})",
            object.object_id, object.definition_id, object.tile_position.x, object.tile_position.y
        ),
        GameEvent::WorldObjectRemoved { object_id } => {
            debug!("client projected object removed: {}", object_id)
        }
        GameEvent::RemotePlayerUpserted { player } => debug!(
            "client remote player upserted: {} object {} at ({}, {})",
            player.player_id.0, player.object_id, player.tile_position.x, player.tile_position.y
        ),
        GameEvent::RemotePlayerRemoved { player_id } => {
            debug!("client remote player removed: {}", player_id.0)
        }
        GameEvent::FloorMapReplaced {
            space_id,
            z,
            width,
            height,
            ..
        } => info!(
            "client floor map replaced: space {} z={} dims {}x{}",
            space_id.0, z, width, height
        ),
        GameEvent::FloorTileSet {
            space_id,
            z,
            x,
            y,
            floor_type,
        } => debug!(
            "client floor tile set: space {} z={} ({},{}) -> {:?}",
            space_id.0, z, x, y, floor_type
        ),
        GameEvent::WorldTimeChanged { time_of_day } => debug!(
            "client world time updated: {:.4} -> {:.4}",
            client_state.world_time, time_of_day
        ),
        GameEvent::PlayerExperienceChanged { experience } => info!(
            "client player experience updated: lvl {} xp {}",
            experience.level, experience.current_xp
        ),
        GameEvent::PlayerClassChanged { class } => {
            info!("client player class set: {:?}", class)
        }
        GameEvent::PlayerAppearanceChanged { .. } => {
            debug!("client player appearance updated")
        }
        GameEvent::PlayerAttributesChanged { attributes } => {
            debug!(
                "client attributes: STR {} AGI {} CON {} WIL {} CHA {} FOC {}",
                attributes.strength,
                attributes.agility,
                attributes.constitution,
                attributes.willpower,
                attributes.charisma,
                attributes.focus
            )
        }
        GameEvent::PlayerCombatStatsChanged { stats } => {
            debug!(
                "client combat stats: atk {}-{} {:?} to-hit {:+} DC {} armor {} block {} ({}%) shield={}",
                stats.damage_min,
                stats.damage_max,
                stats.damage_type,
                stats.attack_bonus,
                stats.dodge_dc,
                stats.armor,
                stats.block,
                stats.block_chance_pct,
                stats.has_shield,
            )
        }
        GameEvent::TradeStateChanged { state } => match state {
            None => debug!("client trade state cleared"),
            Some(view) => debug!(
                "client trade state: session {} partner {:?} ({}) us={} them={} ready={}/{} confirm={}/{}",
                view.session_id,
                view.partner_kind,
                view.partner_name,
                view.our_offers.len(),
                view.their_offers.len(),
                view.our_ready,
                view.their_ready,
                view.our_confirmed,
                view.their_confirmed
            ),
        },
        GameEvent::PartyStateChanged { party } => match party {
            None => debug!("client party state cleared"),
            Some(view) => debug!(
                "client party state: party {} leader {:?} members={} focus={:?}",
                view.party_id,
                view.leader,
                view.members.len(),
                view.focus_target
            ),
        },
        GameEvent::LearnedRecipesChanged { recipes } => info!(
            "client learned recipes replaced: {} -> {} known",
            client_state.learned_recipes.len(),
            recipes.len()
        ),
        GameEvent::LogStateChanged { state } => debug!(
            "client log state replaced: {} sections, {} subentries",
            state.sections.len(),
            state.subentry_count(),
        ),
        GameEvent::SkillSheetChanged {
            ranks,
            available_points,
            available_ability_bumps,
        } => debug!(
            "client skill sheet replaced: ranks {:?} points {} bumps {}",
            ranks, available_points, available_ability_bumps
        ),
        GameEvent::DiscoveredTilesReplaced { tiles } => {
            let total: usize = tiles.values().map(|v| v.len()).sum();
            info!(
                "client discovered tiles replaced: {} space(s), {} tile(s) total",
                tiles.len(),
                total
            )
        }
        GameEvent::TilesDiscovered { space_id, tiles } => debug!(
            "client tiles discovered: space {} +{} tile(s)",
            space_id.0,
            tiles.len()
        ),
    }
}

/// Returns `true` when the `current` set of magical effects differs from
/// `prev` enough to warrant emitting `PlayerEffectsChanged`. Compared
/// element-wise (order is stable on the server: `apply` pushes to the end,
/// `tick_magic_effects` uses `retain` which preserves order), so multiple
/// entries of the same kind are distinguished correctly. Membership,
/// magnitude (epsilon), remaining-seconds at integer resolution, and the
/// optional `secondary_magnitude` (for Chill) all count.
#[cfg(feature = "server-sim")]
fn effects_diff(prev: &[ClientActiveEffect], current: &[ClientActiveEffect]) -> bool {
    if prev.len() != current.len() {
        return true;
    }
    for (p, c) in prev.iter().zip(current.iter()) {
        if p.kind != c.kind
            || (p.magnitude - c.magnitude).abs() > f32::EPSILON
            || p.remaining_seconds.floor() != c.remaining_seconds.floor()
        {
            return true;
        }
        match (p.secondary_magnitude, c.secondary_magnitude) {
            (None, None) => {}
            (Some(a), Some(b)) if (a - b).abs() <= f32::EPSILON => {}
            _ => return true,
        }
    }
    false
}
