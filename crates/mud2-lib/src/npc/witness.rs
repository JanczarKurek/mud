//! Witnessed crimes — the immediate-reaction layer of the justice model.
//!
//! The guilt ledger (`npc::guilt`) is the *slow* channel: attacking a faction
//! member raises the attacker's standing with the whole faction, but nothing
//! draws steel until the Wanted threshold. This module is the *fast* channel:
//! every attributed hit on a faction-bearing NPC files a [`CrimeReport`] into
//! a lingering [`CrimeLog`], and NPCs that can actually *see* the assault
//! react on their next AI tick — a `Protector` (town guard) attacks the
//! aggressor on the spot regardless of guilt tier, and an unarmed faction-mate
//! (villager) scatters.
//!
//! Modeled on `world::noise::NoiseField`: a decaying resource queue rather
//! than Bevy events, because NPC AI ticks on per-NPC timers and a crime
//! committed between two ticks must not be missed. Server-authoritative,
//! never persisted, never replicated — pure simulation state.

use bevy::prelude::*;

use crate::npc::guilt::FactionMask;
use crate::world::components::{SpaceId, TilePosition};

/// How long a crime stays witnessable. Sized to outlive the gap between an
/// NPC's AI ticks with room to spare, so a guard mid-stride still reacts to an
/// assault it walked in on. `[tunable]`
pub const CRIME_LIFETIME_SECONDS: f32 = 5.0;

/// Audible radius (in tiles) of a protector raising the alarm. Louder than
/// `ATTACK_NOISE` — a shout is meant to carry. Out-of-sight wandering guards
/// that hear it go `Alert` at the shouter's tile via the existing noise path.
/// `[tunable]`
pub const ALARM_NOISE: i32 = 14;

/// One attributed assault on a faction-bearing NPC.
#[derive(Clone, Copy, Debug)]
pub struct CrimeReport {
    /// Who swung — a player or an NPC (a wolf mauling a sheep counts).
    pub attacker: Entity,
    pub victim: Entity,
    /// Where the assault landed. Witnesses that can see this tile count as
    /// having seen the crime even if the attacker has since ducked away.
    pub victim_tile: TilePosition,
    /// Copied out at damage time — a killed victim is despawned before any
    /// witness ticks.
    pub victim_factions: FactionMask,
    pub space_id: SpaceId,
}

/// Queue of this frame's crimes. Pushed by `apply_pending_damage`, drained
/// into [`CrimeLog`] by [`update_crime_log`].
#[derive(Resource, Default)]
pub struct PendingCrimes {
    pub items: Vec<CrimeReport>,
}

impl PendingCrimes {
    pub fn push(&mut self, report: CrimeReport) {
        self.items.push(report);
    }
}

#[derive(Clone, Copy, Debug)]
struct ActiveCrime {
    report: CrimeReport,
    remaining_seconds: f32,
}

/// Decaying set of recent crimes that NPCs sample each AI tick. A handful of
/// entries at most, so a linear `Vec` (like `NoiseField`) beats a map.
#[derive(Resource, Default)]
pub struct CrimeLog {
    active: Vec<ActiveCrime>,
}

impl CrimeLog {
    /// Every live crime in `space_id`.
    pub fn iter_space(&self, space_id: SpaceId) -> impl Iterator<Item = &CrimeReport> {
        self.active
            .iter()
            .filter(move |c| c.report.space_id == space_id)
            .map(|c| &c.report)
    }

    pub fn clear(&mut self) {
        self.active.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.active.len()
    }
}

/// Marks an NPC as a protector of the given social factions: witnessing an
/// attributed attack on a member makes it attack the aggressor immediately,
/// independent of the guilt ledger. Resolved at spawn from the definition's
/// `protects_factions:` list (see `resolve_npc_tag_components`) — template
/// data, never persisted.
#[derive(Component, Clone, Copy, Debug)]
pub struct Protector {
    pub protects: FactionMask,
}

/// Decays the lingering [`CrimeLog`] and folds in this frame's
/// [`PendingCrimes`]. Registered after `apply_pending_damage` (the sole
/// producer) and gated on `simulation_active`.
pub fn update_crime_log(
    time: Res<Time>,
    mut pending: ResMut<PendingCrimes>,
    mut log: ResMut<CrimeLog>,
) {
    let dt = time.delta_secs();
    // Decay first so freshly-filed crimes keep their full lifetime.
    if dt > 0.0 {
        for crime in &mut log.active {
            crime.remaining_seconds -= dt;
        }
        log.active.retain(|c| c.remaining_seconds > 0.0);
    }

    for report in pending.items.drain(..) {
        // Merge per (attacker, victim): a DoT tick or a flurry of fast swings
        // is one ongoing assault, not a stack of separate crimes. Refresh the
        // timer and track the latest position of the offense.
        if let Some(existing) = log
            .active
            .iter_mut()
            .find(|c| c.report.attacker == report.attacker && c.report.victim == report.victim)
        {
            existing.report = report;
            existing.remaining_seconds = CRIME_LIFETIME_SECONDS;
        } else {
            log.active.push(ActiveCrime {
                report,
                remaining_seconds: CRIME_LIFETIME_SECONDS,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn tile(x: i32, y: i32) -> TilePosition {
        TilePosition { x, y, z: 0 }
    }

    fn report(attacker: Entity, victim: Entity, x: i32) -> CrimeReport {
        CrimeReport {
            attacker,
            victim,
            victim_tile: tile(x, 0),
            victim_factions: crate::npc::hostility::TagMask(0b1),
            space_id: SpaceId(1),
        }
    }

    fn app_with_log() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<PendingCrimes>();
        app.init_resource::<CrimeLog>();
        app.add_systems(Update, update_crime_log);
        // First update initializes Time.
        app.update();
        // Lift the virtual clock's 250ms max_delta clamp so multi-second
        // manual steps arrive as one delta.
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(Duration::MAX);
        app
    }

    fn advance(app: &mut App, seconds: f32) {
        // Manual time stepping: `Time<Virtual>::advance_by` alone doesn't
        // change the *delta* the next update computes (that comes from the
        // real clock), and this system decays by delta.
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f32(seconds),
        ));
        app.update();
    }

    #[test]
    fn crimes_fold_and_decay() {
        let mut app = app_with_log();
        let attacker = Entity::from_raw_u32(1).unwrap();
        let victim = Entity::from_raw_u32(2).unwrap();
        app.world_mut()
            .resource_mut::<PendingCrimes>()
            .push(report(attacker, victim, 0));
        advance(&mut app, 0.1);
        let log = app.world().resource::<CrimeLog>();
        assert_eq!(log.len(), 1);
        assert_eq!(log.iter_space(SpaceId(1)).count(), 1);
        assert_eq!(log.iter_space(SpaceId(2)).count(), 0);

        // Fully decays after the lifetime.
        advance(&mut app, CRIME_LIFETIME_SECONDS + 0.1);
        assert_eq!(app.world().resource::<CrimeLog>().len(), 0);
    }

    #[test]
    fn repeat_offense_merges_and_refreshes() {
        let mut app = app_with_log();
        let attacker = Entity::from_raw_u32(1).unwrap();
        let victim = Entity::from_raw_u32(2).unwrap();
        app.world_mut()
            .resource_mut::<PendingCrimes>()
            .push(report(attacker, victim, 0));
        advance(&mut app, CRIME_LIFETIME_SECONDS - 1.0);
        // Second hit on the same victim: merged, timer refreshed, tile updated.
        app.world_mut()
            .resource_mut::<PendingCrimes>()
            .push(report(attacker, victim, 5));
        advance(&mut app, 0.1);
        {
            let log = app.world().resource::<CrimeLog>();
            assert_eq!(log.len(), 1);
            assert_eq!(
                log.iter_space(SpaceId(1)).next().unwrap().victim_tile,
                tile(5, 0)
            );
        }
        // Well past the original lifetime but inside the refreshed one.
        advance(&mut app, CRIME_LIFETIME_SECONDS - 1.0);
        assert_eq!(app.world().resource::<CrimeLog>().len(), 1);

        // A different victim is a distinct crime.
        let other = Entity::from_raw_u32(3).unwrap();
        app.world_mut()
            .resource_mut::<PendingCrimes>()
            .push(report(attacker, other, 0));
        advance(&mut app, 0.1);
        assert_eq!(app.world().resource::<CrimeLog>().len(), 2);
    }
}
