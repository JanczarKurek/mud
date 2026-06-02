//! Unified damage application and death resolution.
//!
//! All damage in the game flows through `PendingDamageEvents`. Damage-producing
//! systems (melee/ranged in `resolve_battle_turn`, spells in
//! `handle_cast_spell_at`, DoT ticks in `tick_dot_effects`, and any future
//! source — summons, traps, environment hazards) push events here instead of
//! mutating `VitalStats::health` directly. `apply_pending_damage` is the sole
//! writer of damage-driven health changes and the sole place that handles
//! deaths.
//!
//! Attribution rule: **last hit wins**. The source of the damage event that
//! brings HP to 0 receives kill credit. Indirect sources (DoTs, summons,
//! placed obstacles) carry the responsible player via
//! `DamageSource::OwnedByPlayer` so they award XP exactly like a direct hit.
//! NPC-on-NPC and environment damage carry no XP credit.
//!
//! Server-authoritative. None of these types cross the wire; clients observe
//! deaths only through the resulting `GameEvent::PlayerVitalsChanged`,
//! `GameEvent::ExperienceGained`, and existing UI events.

use bevy::prelude::*;

use crate::combat::damage_type::DamageType;
use crate::game::resources::{GameUiEvent, PendingGameUiEvents, VfxAnchor};
use crate::magic::effects::MagicEffects;
use crate::magic::resources::{EffectKind, SpellDefinitions};
use crate::npc::components::Npc;
use crate::player::components::{ChatLog, Player, PlayerId, PlayerIdentity, VitalStats};
use crate::player::lifecycle::{PendingPlayerDeath, PendingPlayerDeaths};
use crate::player::progression::{xp_grant_for_kill, Experience, PendingXpGrant, PendingXpGrants};
use crate::quest::events::{PendingQuestEvents, QuestEvent};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::loot::spawn_corpse_for_npc;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;

/// Origin of a single damage application. Drives both XP attribution and
/// behaviour gates (e.g. NPC-on-NPC damage awards nothing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DamageSource {
    /// Direct player attack — melee, ranged, or targeted spell.
    Player(PlayerId),
    /// Indirect player-owned damage — DoT tick, summon attack, placed trap.
    OwnedByPlayer(PlayerId),
    /// NPC attacker. Carries the entity so future hate-list / aggro work can
    /// resolve it; never grants XP.
    Npc { entity: Entity },
    /// Lava, fall damage, etc. — unattributed.
    Environment,
}

impl DamageSource {
    /// The `PlayerId` that should receive XP credit if this damage delivers
    /// the killing blow. `None` for NPC and environment sources.
    pub fn xp_credit(&self) -> Option<PlayerId> {
        match self {
            Self::Player(id) | Self::OwnedByPlayer(id) => Some(*id),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DamageEvent {
    pub target: Entity,
    pub amount: f32,
    pub source: DamageSource,
    pub damage_type: DamageType,
    /// Optional override for the hit VFX. When `None`, the drainer falls back
    /// to `damage_type.default_hit_vfx_id()`.
    pub vfx_override: Option<String>,
}

#[derive(Resource, Default)]
pub struct PendingDamageEvents {
    pub events: Vec<DamageEvent>,
}

impl PendingDamageEvents {
    pub fn push(&mut self, event: DamageEvent) {
        self.events.push(event);
    }
}

type DamageTargetQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut VitalStats,
        &'static SpaceResident,
        &'static TilePosition,
        &'static OverworldObject,
        Option<&'static Player>,
        Option<&'static Npc>,
        Option<&'static Experience>,
        Option<&'static mut MagicEffects>,
    ),
>;

/// Drains `PendingDamageEvents`, applies the damage, and runs death handling
/// in one place. Registered after every damage producer and before
/// `collect_game_events_from_authority`.
#[allow(clippy::too_many_arguments)]
pub fn apply_pending_damage(
    mut pending: ResMut<PendingDamageEvents>,
    mut targets: DamageTargetQuery,
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    spell_definitions: Res<SpellDefinitions>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut quest_events: ResMut<PendingQuestEvents>,
    mut pending_player_deaths: ResMut<PendingPlayerDeaths>,
    mut pending_xp_grants: ResMut<PendingXpGrants>,
    mut chat_log_query: Query<&mut ChatLog, With<Player>>,
    player_identity_query: Query<&PlayerIdentity, With<Player>>,
    mut commands: Commands,
) {
    if pending.events.is_empty() {
        return;
    }
    let events = std::mem::take(&mut pending.events);

    for event in events {
        let Ok((
            mut target_vitals,
            target_space,
            target_position,
            target_object,
            is_player,
            is_npc,
            target_experience,
            mut target_effects,
        )) = targets.get_mut(event.target)
        else {
            continue;
        };
        if target_vitals.health <= 0.0 {
            continue;
        }
        if event.amount <= 0.0 {
            continue;
        }

        target_vitals.health = (target_vitals.health - event.amount).max(0.0);

        // Damage wakes a sleeping target — any damage source, not just
        // melee. This used to live in `tick_battle` and only fired for
        // melee/ranged hits, so spell damage (e.g. goblin mage casting at
        // a slept player) silently kept the target asleep.
        if let Some(effects) = target_effects.as_mut() {
            effects.clear(EffectKind::Sleep);
        }

        if target_vitals.health > 0.0 {
            // Survivor: emit the damage-type-keyed hit VFX. Death plays
            // `death_poof` below instead, so we don't stack two effects on
            // the killing blow.
            let vfx_id = event
                .vfx_override
                .clone()
                .unwrap_or_else(|| event.damage_type.default_hit_vfx_id().to_owned());
            ui_events.push_broadcast(GameUiEvent::VfxSpawn {
                definition_id: vfx_id,
                anchor: VfxAnchor::follow(target_object.object_id),
            });
            continue;
        }

        // Snapshot what we need before despawning / dropping the borrow.
        let space_id = target_space.space_id;
        let position = *target_position;
        let definition_id = target_object.definition_id.clone();
        let object_id = target_object.object_id;
        let level = target_experience.map(|exp| exp.level).unwrap_or(1);
        let target_name = object_registry
            .display_name(object_id, &definitions, &spell_definitions)
            .unwrap_or_else(|| definition_id.clone());
        let is_player_target = is_player.is_some();
        let is_npc_target = is_npc.is_some();

        if is_npc_target {
            if let Some(loot_table) = definitions
                .get(&definition_id)
                .and_then(|def| def.loot_table.as_ref())
            {
                spawn_corpse_for_npc(
                    &mut commands,
                    &definitions,
                    &mut object_registry,
                    loot_table,
                    space_id,
                    position,
                );
            }
            let killer_player_id = event.source.xp_credit().map(|id| id.0);
            quest_events.events.push(QuestEvent::ObjectKilled {
                type_id: definition_id.clone(),
                killer_player_id,
            });
            if let Some(player_id) = event.source.xp_credit() {
                let amount = xp_grant_for_kill(level);
                pending_xp_grants
                    .grants
                    .push(PendingXpGrant { player_id, amount });
                let killer_name = player_identity_query
                    .iter()
                    .find(|identity| identity.id == player_id)
                    .map(|identity| identity.display_name.clone())
                    .unwrap_or_else(|| format!("Player#{}", player_id.0));
                broadcast_chat_line(
                    &mut chat_log_query,
                    format!("[{killer_name} gained {amount} XP]"),
                );
            }
            ui_events.push_broadcast(GameUiEvent::VfxSpawn {
                definition_id: "death_poof".to_owned(),
                anchor: VfxAnchor::tile(space_id, position),
            });
            commands.entity(event.target).despawn();
            broadcast_chat_line(&mut chat_log_query, format!("[{target_name} dies]"));
        } else if is_player_target {
            broadcast_chat_line(&mut chat_log_query, format!("[{target_name} is defeated]"));
            pending_player_deaths.deaths.push(PendingPlayerDeath {
                entity: event.target,
                space_id,
                tile_position: position,
                name: target_name,
            });
        }
    }
}

fn broadcast_chat_line(chat_log_query: &mut Query<&mut ChatLog, With<Player>>, message: String) {
    for mut chat_log in chat_log_query.iter_mut() {
        chat_log.push_line(message.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xp_credit_resolves_for_player_and_owned_sources() {
        let p = PlayerId(42);
        assert_eq!(DamageSource::Player(p).xp_credit(), Some(p));
        assert_eq!(DamageSource::OwnedByPlayer(p).xp_credit(), Some(p));
    }

    #[test]
    fn xp_credit_none_for_npc_and_environment() {
        assert_eq!(
            DamageSource::Npc {
                entity: Entity::PLACEHOLDER,
            }
            .xp_credit(),
            None
        );
        assert_eq!(DamageSource::Environment.xp_credit(), None);
    }
}
