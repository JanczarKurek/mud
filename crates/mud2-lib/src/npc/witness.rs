//! Witnessed crimes — where offenses become *records* and reactions start.
//!
//! Every attributed hit on a faction-bearing NPC files a [`CrimeReport`] into
//! the lingering [`CrimeLog`]. From there, two things happen:
//!
//! - **Fast channel** (unchanged): NPCs that can actually *see* the assault
//!   react on their next AI tick — a `Protector` (town guard) attacks the
//!   aggressor on the spot regardless of guilt tier, and an unarmed
//!   faction-mate (villager) scatters.
//! - **Slow channel** (witness-gated guilt): player-attributed reports mint a
//!   [`CrimeRecord`] here — the sole place with the dedup context to decide
//!   "same scuffle" vs "fresh offense" — and the record reaches NPC memories
//!   only through the surviving victim (pushed here) and actual witnesses
//!   (resolved per-NPC in `update_roaming_npcs`). If nobody qualifies before
//!   the log entry decays, the crime never happened as far as the world knows.
//!
//! Modeled on `world::noise::NoiseField`: a decaying resource queue rather
//! than Bevy events, because NPC AI ticks on per-NPC timers and a crime
//! committed between two ticks must not be missed. Server-authoritative,
//! never persisted, never replicated — pure simulation state.

use bevy::prelude::*;

use crate::npc::guilt::{
    CrimeIdAllocator, CrimeKind, CrimeRecord, FactionInterner, FactionMask, PendingCrimeLearns,
    ATTACK_DEBOUNCE_SECONDS,
};
use crate::player::components::PlayerId;
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
#[derive(Clone, Debug)]
pub struct CrimeReport {
    /// Who swung — a player or an NPC (a wolf mauling a sheep counts).
    pub attacker: Entity,
    /// The player behind the swing, when there is one. Only these reports
    /// mint [`CrimeRecord`]s — NPC-on-NPC violence is reacted to but never
    /// remembered as guilt.
    pub attacker_player: Option<PlayerId>,
    pub victim: Entity,
    /// Whether the blow was survivable or the kill itself.
    pub kind: CrimeKind,
    /// The victim's display name at report time, carried into the record —
    /// a killed victim is despawned before the record is read anywhere.
    pub victim_name: String,
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

#[derive(Clone, Debug)]
pub struct ActiveCrime {
    pub report: CrimeReport,
    remaining_seconds: f32,
    /// The guilt record minted for this entry, when the attacker is a player.
    /// Witnesses copy it out of here; if the entry decays unlearned, the
    /// crime leaves no trace.
    pub record: Option<CrimeRecord>,
    /// When `record` was minted. A follow-up hit inside
    /// [`ATTACK_DEBOUNCE_SECONDS`] of this reuses the record (one scuffle);
    /// past it, a sustained beating mints a fresh one.
    record_minted_at: f32,
}

/// Decaying set of recent crimes that NPCs sample each AI tick. A handful of
/// entries at most, so a linear `Vec` (like `NoiseField`) beats a map.
#[derive(Resource, Default)]
pub struct CrimeLog {
    active: Vec<ActiveCrime>,
}

impl CrimeLog {
    /// Every live crime in `space_id`.
    pub fn iter_space(&self, space_id: SpaceId) -> impl Iterator<Item = &ActiveCrime> {
        self.active
            .iter()
            .filter(move |c| c.report.space_id == space_id)
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

/// Decays the lingering [`CrimeLog`], folds in this frame's [`PendingCrimes`],
/// mints [`CrimeRecord`]s for player-attributed reports, and hands the record
/// straight to a surviving victim (who knows first-hand; no witness gate).
///
/// Registered after `apply_pending_damage` (the sole producer) and gated on
/// `simulation_active`.
pub fn update_crime_log(
    time: Res<Time>,
    mut pending: ResMut<PendingCrimes>,
    mut log: ResMut<CrimeLog>,
    interner: Res<FactionInterner>,
    mut crime_ids: ResMut<CrimeIdAllocator>,
    mut learns: ResMut<PendingCrimeLearns>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
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
            existing.remaining_seconds = CRIME_LIFETIME_SECONDS;
            if let Some(player) = report.attacker_player {
                let inside_window = existing
                    .record
                    .as_ref()
                    .is_some_and(|_| now - existing.record_minted_at < ATTACK_DEBOUNCE_SECONDS);
                match (&mut existing.record, inside_window) {
                    (Some(record), true) => {
                        // Same scuffle. A kill upgrades the record in place —
                        // murder subsumes the assault it grew out of (same id,
                        // so witnesses who saw the assault re-learn the worse
                        // truth; `CrimeMemory::learn` replaces on upgrade).
                        if report.kind == CrimeKind::Kill && record.kind != CrimeKind::Kill {
                            record.kind = CrimeKind::Kill;
                        }
                    }
                    _ => {
                        // Past the window (or an entry that never had a
                        // record): a sustained beating is a fresh offense.
                        let record = mint_record(&mut crime_ids, &interner, player, &report);
                        if report.kind == CrimeKind::Attack {
                            learns.push(report.victim, record.clone());
                        }
                        existing.record = Some(record);
                        existing.record_minted_at = now;
                    }
                }
            }
            existing.report = report;
        } else {
            let record = report.attacker_player.map(|player| {
                let record = mint_record(&mut crime_ids, &interner, player, &report);
                // The surviving victim knows first-hand — no LoS gate. A
                // kill's victim is despawned, so there is nobody to tell.
                if report.kind == CrimeKind::Attack {
                    learns.push(report.victim, record.clone());
                }
                record
            });
            log.active.push(ActiveCrime {
                report,
                remaining_seconds: CRIME_LIFETIME_SECONDS,
                record,
                record_minted_at: now,
            });
        }
    }
}

fn mint_record(
    crime_ids: &mut CrimeIdAllocator,
    interner: &FactionInterner,
    player: PlayerId,
    report: &CrimeReport,
) -> CrimeRecord {
    CrimeRecord {
        id: crime_ids.allocate(),
        player,
        kind: report.kind,
        victim_name: report.victim_name.clone(),
        victim_factions: interner.names_for_mask(report.victim_factions),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::npc::guilt::{ATTACK_GUILT, KILL_GUILT};

    fn tile(x: i32, y: i32) -> TilePosition {
        TilePosition { x, y, z: 0 }
    }

    fn interner() -> FactionInterner {
        FactionInterner::build(["emberbrook_watch"].into_iter())
    }

    fn report_of(attacker: Entity, victim: Entity, x: i32, kind: CrimeKind) -> CrimeReport {
        CrimeReport {
            attacker,
            attacker_player: Some(PlayerId(1)),
            victim,
            kind,
            victim_name: "Bob".to_owned(),
            victim_tile: tile(x, 0),
            victim_factions: interner().resolve(&["emberbrook_watch".to_owned()]),
            space_id: SpaceId(1),
        }
    }

    fn report(attacker: Entity, victim: Entity, x: i32) -> CrimeReport {
        report_of(attacker, victim, x, CrimeKind::Attack)
    }

    fn app_with_log() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<PendingCrimes>();
        app.init_resource::<CrimeLog>();
        app.init_resource::<CrimeIdAllocator>();
        app.init_resource::<PendingCrimeLearns>();
        app.insert_resource(interner());
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

    fn push(app: &mut App, report: CrimeReport) {
        app.world_mut().resource_mut::<PendingCrimes>().push(report);
    }

    fn records(app: &App) -> Vec<CrimeRecord> {
        app.world()
            .resource::<CrimeLog>()
            .active
            .iter()
            .filter_map(|c| c.record.clone())
            .collect()
    }

    fn victim_learns(app: &App) -> Vec<(Entity, CrimeRecord)> {
        app.world().resource::<PendingCrimeLearns>().items.clone()
    }

    #[test]
    fn crimes_fold_and_decay() {
        let mut app = app_with_log();
        let attacker = Entity::from_raw_u32(1).unwrap();
        let victim = Entity::from_raw_u32(2).unwrap();
        push(&mut app, report(attacker, victim, 0));
        advance(&mut app, 0.1);
        let log = app.world().resource::<CrimeLog>();
        assert_eq!(log.len(), 1);
        assert_eq!(log.iter_space(SpaceId(1)).count(), 1);
        assert_eq!(log.iter_space(SpaceId(2)).count(), 0);

        // Fully decays after the lifetime — record and all: the unwitnessed
        // crime never happened.
        advance(&mut app, CRIME_LIFETIME_SECONDS + 0.1);
        assert_eq!(app.world().resource::<CrimeLog>().len(), 0);
    }

    #[test]
    fn repeat_offense_merges_and_refreshes() {
        let mut app = app_with_log();
        let attacker = Entity::from_raw_u32(1).unwrap();
        let victim = Entity::from_raw_u32(2).unwrap();
        push(&mut app, report(attacker, victim, 0));
        advance(&mut app, CRIME_LIFETIME_SECONDS - 1.0);
        // Second hit on the same victim: merged, timer refreshed, tile updated.
        push(&mut app, report(attacker, victim, 5));
        advance(&mut app, 0.1);
        {
            let log = app.world().resource::<CrimeLog>();
            assert_eq!(log.len(), 1);
            assert_eq!(
                log.iter_space(SpaceId(1))
                    .next()
                    .unwrap()
                    .report
                    .victim_tile,
                tile(5, 0)
            );
        }
        // Well past the original lifetime but inside the refreshed one.
        advance(&mut app, CRIME_LIFETIME_SECONDS - 1.0);
        assert_eq!(app.world().resource::<CrimeLog>().len(), 1);

        // A different victim is a distinct crime.
        let other = Entity::from_raw_u32(3).unwrap();
        push(&mut app, report(attacker, other, 0));
        advance(&mut app, 0.1);
        assert_eq!(app.world().resource::<CrimeLog>().len(), 2);
    }

    #[test]
    fn a_flurry_mints_one_record_a_sustained_beating_escalates() {
        let mut app = app_with_log();
        let attacker = Entity::from_raw_u32(1).unwrap();
        let victim = Entity::from_raw_u32(2).unwrap();

        push(&mut app, report(attacker, victim, 0));
        advance(&mut app, 0.1);
        push(&mut app, report(attacker, victim, 0));
        advance(&mut app, 0.1);
        let minted = records(&app);
        assert_eq!(minted.len(), 1, "two hits inside 3s are one crime");
        assert_eq!(minted[0].kind, CrimeKind::Attack);
        assert_eq!(minted[0].victim_factions, vec!["emberbrook_watch"]);

        // Past the debounce window (entry still alive): a fresh record.
        advance(&mut app, ATTACK_DEBOUNCE_SECONDS);
        push(&mut app, report(attacker, victim, 0));
        advance(&mut app, 0.1);
        let minted = records(&app);
        assert_eq!(minted.len(), 1, "still one entry, record replaced");
        assert_ne!(minted[0].id, 1, "but it is a new crime id");
        assert_eq!(
            minted[0].kind.points(),
            ATTACK_GUILT,
            "each 3s of a beating charges another assault"
        );
    }

    #[test]
    fn a_kill_upgrades_the_scuffles_record_in_place() {
        let mut app = app_with_log();
        let attacker = Entity::from_raw_u32(1).unwrap();
        let victim = Entity::from_raw_u32(2).unwrap();

        push(&mut app, report(attacker, victim, 0));
        advance(&mut app, 0.1);
        let assault_id = records(&app)[0].id;
        push(&mut app, report_of(attacker, victim, 0, CrimeKind::Kill));
        advance(&mut app, 0.1);

        let minted = records(&app);
        assert_eq!(minted.len(), 1);
        assert_eq!(minted[0].id, assault_id, "same engagement, same id");
        assert_eq!(minted[0].kind, CrimeKind::Kill);
        assert_eq!(minted[0].kind.points(), KILL_GUILT);
    }

    #[test]
    fn the_surviving_victim_learns_first_hand_a_dead_one_cannot() {
        let mut app = app_with_log();
        let attacker = Entity::from_raw_u32(1).unwrap();
        let victim = Entity::from_raw_u32(2).unwrap();
        let slain = Entity::from_raw_u32(3).unwrap();

        push(&mut app, report(attacker, victim, 0));
        // Straight-out kill of another NPC (one-shot, no prior assault).
        push(&mut app, report_of(attacker, slain, 4, CrimeKind::Kill));
        advance(&mut app, 0.1);

        let learns = victim_learns(&app);
        assert_eq!(learns.len(), 1, "only the survivor learns");
        assert_eq!(learns[0].0, victim);
        assert_eq!(learns[0].1.kind, CrimeKind::Attack);
        // The kill still minted a record — witnesses can pick it up.
        assert_eq!(records(&app).len(), 2);
    }

    #[test]
    fn npc_attackers_mint_no_records() {
        let mut app = app_with_log();
        let wolf = Entity::from_raw_u32(1).unwrap();
        let sheep = Entity::from_raw_u32(2).unwrap();
        let mut report = report(wolf, sheep, 0);
        report.attacker_player = None;
        push(&mut app, report);
        advance(&mut app, 0.1);

        assert_eq!(
            app.world().resource::<CrimeLog>().len(),
            1,
            "still reacted to"
        );
        assert!(records(&app).is_empty(), "but never remembered as guilt");
        assert!(victim_learns(&app).is_empty());
    }
}
