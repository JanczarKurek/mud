//! Deferred spell resolution — the engine behind flying missiles and
//! time-shaped AoE (spreading fire, spiraling electricity).
//!
//! Both features need the same missing primitive: *"resolve this spell effect
//! N seconds from now."* A cast that flies or spreads pushes one or more
//! [`ScheduledImpact`]s onto [`ScheduledImpacts`] instead of dealing damage
//! immediately; [`tick_scheduled_impacts`] counts each down and, on expiry,
//! turns it into ordinary `DamageEvent`s and `VfxSpawn`s — so all damage still
//! flows through the single authoritative `apply_pending_damage` writer.
//!
//! Server-authoritative. The queue is a transient resource: it is never saved
//! (in-flight missiles simply drop on restart, like the client projectiles
//! already do) and never crosses the wire (it only emits the existing event
//! channels). It lives in `CombatPlugin` so its ordering against
//! `apply_pending_damage` is intra-plugin.

use bevy::prelude::*;

use crate::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use crate::combat::damage_type::DamageType;
use crate::game::resources::{GameUiEvent, PendingGameUiEvents, VfxAnchor};
use crate::magic::effects::MagicEffects;
use crate::magic::resources::EffectSpec;
use crate::npc::components::Npc;
use crate::player::components::Player;
use crate::world::components::{SpaceId, SpaceResident, TilePosition};

/// A single spell effect that resolves once its countdown elapses.
#[derive(Clone, Debug)]
pub struct ScheduledImpact {
    /// Seconds until this impact resolves. Decremented each frame; resolves
    /// once it reaches `<= 0` (an impact scheduled at delay 0 resolves the same
    /// frame it was created, before `apply_pending_damage` runs).
    pub remaining_seconds: f32,
    pub space_id: SpaceId,
    pub damage: f32,
    pub damage_type: DamageType,
    /// Carries the caster's `PlayerId` for XP attribution / buff ownership.
    pub source: DamageSource,
    pub kind: ImpactKind,
}

/// Whether an impact hits a locked entity (single-target homing missile) or a
/// fixed tile (one cell of an AoE footprint; patterns decompose into many).
#[derive(Clone, Debug)]
pub enum ImpactKind {
    /// Single-target homing missile: damage `target` on impact wherever it is.
    /// A despawned/dead target is a clean no-op — `apply_pending_damage` guards
    /// missing/dead entities — so no validity check is needed here.
    Locked {
        target: Entity,
        hit_vfx: Option<String>,
        /// Debuffs applied to the target on impact (NPCs only).
        buffs: Vec<EffectSpec>,
    },
    /// One AoE tile. Damages every resident standing exactly on `tile` (planar
    /// at the target floor) and flashes `vfx_on_tile` there.
    Point {
        tile: TilePosition,
        hit_vfx: Option<String>,
        vfx_on_tile: Option<String>,
        /// Debuffs applied to NPCs on this tile.
        buffs: Vec<EffectSpec>,
    },
}

#[derive(Resource, Default)]
pub struct ScheduledImpacts {
    pub items: Vec<ScheduledImpact>,
}

impl ScheduledImpacts {
    pub fn push(&mut self, impact: ScheduledImpact) {
        self.items.push(impact);
    }
}

/// Minimum missile flight time, so an adjacent cast still shows a brief flight
/// rather than landing instantly.
pub const MIN_FLIGHT_SECONDS: f32 = 0.08;

/// Flight time for a missile: `distance / speed`, floored at
/// [`MIN_FLIGHT_SECONDS`]. Speed is clamped to a small positive value so a
/// misconfigured `speed: 0` can't divide by zero.
pub fn projectile_travel_seconds(distance_tiles: i32, speed_tiles_per_second: f32) -> f32 {
    let speed = speed_tiles_per_second.max(0.1);
    (distance_tiles.max(0) as f32 / speed).max(MIN_FLIGHT_SECONDS)
}

type ResidentQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static SpaceResident,
        &'static TilePosition,
        Has<Npc>,
    ),
    Or<(With<Npc>, With<Player>)>,
>;
type NpcEffectsQuery<'w, 's> =
    Query<'w, 's, &'static mut MagicEffects, (With<Npc>, Without<Player>)>;

/// Counts down every scheduled impact and resolves the due ones into
/// `PendingDamageEvents` / `PendingGameUiEvents`. Runs after the cast handlers
/// (`process_game_commands`) and before `apply_pending_damage`, so a delay-0
/// impact still lands the same frame with no added latency.
pub fn tick_scheduled_impacts(
    time: Res<Time>,
    mut scheduled: ResMut<ScheduledImpacts>,
    residents: ResidentQuery,
    mut npc_effects: NpcEffectsQuery,
    mut pending_damage: ResMut<PendingDamageEvents>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut commands: Commands,
) {
    if scheduled.items.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let items = std::mem::take(&mut scheduled.items);
    let mut still_pending = Vec::with_capacity(items.len());
    for mut impact in items {
        impact.remaining_seconds -= dt;
        if impact.remaining_seconds > 0.0 {
            still_pending.push(impact);
            continue;
        }
        resolve_impact(
            impact,
            &residents,
            &mut npc_effects,
            &mut pending_damage,
            &mut ui_events,
            &mut commands,
        );
    }
    scheduled.items = still_pending;
}

fn resolve_impact(
    impact: ScheduledImpact,
    residents: &ResidentQuery,
    npc_effects: &mut NpcEffectsQuery,
    pending_damage: &mut PendingDamageEvents,
    ui_events: &mut PendingGameUiEvents,
    commands: &mut Commands,
) {
    let ScheduledImpact {
        space_id,
        damage,
        damage_type,
        source,
        kind,
        ..
    } = impact;
    match kind {
        ImpactKind::Locked {
            target,
            hit_vfx,
            buffs,
        } => {
            // A despawned or de-spatialized target (dead player awaiting
            // respawn) absorbs nothing: the damage would be dropped downstream
            // anyway, but `apply_buffs` writes by entity handle and would
            // re-add effects to a corpse whose MagicEffects were just cleared.
            if residents.get(target).is_err() {
                return;
            }
            if damage > 0.0 {
                pending_damage.push(DamageEvent {
                    target,
                    amount: damage,
                    source,
                    damage_type,
                    vfx_override: hit_vfx,
                });
            }
            if !buffs.is_empty() {
                apply_buffs(target, &buffs, source, npc_effects, commands);
            }
        }
        ImpactKind::Point {
            tile,
            hit_vfx,
            vfx_on_tile,
            buffs,
        } => {
            if let Some(vfx) = vfx_on_tile {
                ui_events.push_broadcast_near(
                    space_id,
                    tile,
                    GameUiEvent::VfxSpawn {
                        definition_id: vfx,
                        anchor: VfxAnchor::tile(space_id, tile),
                    },
                );
            }
            // Exact-tile match — one query, each resident hit at most once.
            // Friendly fire stays on (the caster's tile is fair game), matching
            // the prior inline AoE behavior. Debuffs go to NPCs only.
            for (entity, resident, pos, is_npc) in residents.iter() {
                if resident.space_id != space_id || *pos != tile {
                    continue;
                }
                if damage > 0.0 {
                    pending_damage.push(DamageEvent {
                        target: entity,
                        amount: damage,
                        source,
                        damage_type,
                        vfx_override: hit_vfx.clone(),
                    });
                }
                if is_npc && !buffs.is_empty() {
                    apply_buffs(entity, &buffs, source, npc_effects, commands);
                }
            }
        }
    }
}

fn apply_buffs(
    target: Entity,
    specs: &[EffectSpec],
    source: DamageSource,
    npc_effects: &mut NpcEffectsQuery,
    commands: &mut Commands,
) {
    let mut existing = npc_effects.get_mut(target).ok();
    crate::magic::effects::apply_effects_lazy(
        target,
        specs,
        source.xp_credit(),
        existing.as_deref_mut(),
        commands,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::PlayerId;

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ScheduledImpacts>();
        app.init_resource::<PendingDamageEvents>();
        app.init_resource::<PendingGameUiEvents>();
        app.add_systems(Update, tick_scheduled_impacts);
        app
    }

    fn point_impact(tile: TilePosition, remaining: f32) -> ScheduledImpact {
        ScheduledImpact {
            remaining_seconds: remaining,
            space_id: SpaceId(0),
            damage: 7.0,
            damage_type: DamageType::Fire,
            source: DamageSource::Player(PlayerId(1)),
            kind: ImpactKind::Point {
                tile,
                hit_vfx: None,
                vfx_on_tile: Some("fire_hit".to_owned()),
                buffs: Vec::new(),
            },
        }
    }

    #[test]
    fn due_point_impact_damages_resident_on_its_tile() {
        let mut app = test_app();
        let tile = TilePosition::new(3, 4, 0);
        let npc = app
            .world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: SpaceId(0),
                },
                tile,
            ))
            .id();
        // An entity on a different tile must NOT be hit (exact-tile match).
        let bystander = app
            .world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: SpaceId(0),
                },
                TilePosition::new(9, 9, 0),
            ))
            .id();

        // remaining 0.0 → resolves this frame regardless of dt.
        app.world_mut()
            .resource_mut::<ScheduledImpacts>()
            .push(point_impact(tile, 0.0));
        app.update();

        let damage = app.world().resource::<PendingDamageEvents>();
        assert_eq!(damage.events.len(), 1, "exactly one resident hit");
        assert_eq!(damage.events[0].target, npc);
        assert_eq!(damage.events[0].amount, 7.0);
        let _ = bystander;
        // The impact was consumed.
        assert!(app.world().resource::<ScheduledImpacts>().items.is_empty());
    }

    #[test]
    fn future_impact_stays_pending() {
        let mut app = test_app();
        let tile = TilePosition::new(3, 4, 0);
        app.world_mut().spawn((
            Npc,
            SpaceResident {
                space_id: SpaceId(0),
            },
            tile,
        ));
        app.world_mut()
            .resource_mut::<ScheduledImpacts>()
            .push(point_impact(tile, 10.0));
        app.update();

        assert!(
            app.world()
                .resource::<PendingDamageEvents>()
                .events
                .is_empty(),
            "no damage before the impact is due"
        );
        assert_eq!(
            app.world().resource::<ScheduledImpacts>().items.len(),
            1,
            "impact still pending"
        );
    }

    #[test]
    fn locked_impact_damages_target_even_off_tile() {
        let mut app = test_app();
        // Target sits far from the impact's space-only context; Locked ignores
        // tiles and hits the entity directly (homing missile).
        let target = app
            .world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: SpaceId(0),
                },
                TilePosition::new(50, 50, 0),
            ))
            .id();
        app.world_mut()
            .resource_mut::<ScheduledImpacts>()
            .push(ScheduledImpact {
                remaining_seconds: 0.0,
                space_id: SpaceId(0),
                damage: 5.0,
                damage_type: DamageType::Arcane,
                source: DamageSource::Player(PlayerId(1)),
                kind: ImpactKind::Locked {
                    target,
                    hit_vfx: None,
                    buffs: Vec::new(),
                },
            });
        app.update();

        let damage = app.world().resource::<PendingDamageEvents>();
        assert_eq!(damage.events.len(), 1);
        assert_eq!(damage.events[0].target, target);
        assert_eq!(damage.events[0].amount, 5.0);
    }
}
