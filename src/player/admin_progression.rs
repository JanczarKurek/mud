//! Admin-only progression mutations.
//!
//! Drains the seven `GameCommand::Admin*` progression variants out of
//! `PendingGameCommands` in the `CommandIntercept` set (before
//! `process_game_commands` runs). Each variant targets the player carried in
//! `QueuedGameCommand::player_id` — populated by the admin REPL's
//! `Player.x(...)` methods via `queue_command_for_player`. Commands with no
//! target are silently dropped.
//!
//! Upward XP grants are routed through `PendingXpGrants` so the canonical
//! `apply_xp_grants` pipeline runs (level-ups, skill-point grants, UI
//! toasts). All other variants mutate the matching player directly and emit
//! the corresponding `GameEvent` deltas; the projection layer picks up
//! baseline drift automatically (`SkillSheetChanged`, `PlayerExperienceChanged`).

use bevy::prelude::*;

use crate::game::commands::GameCommand;
use crate::game::resources::{
    GameEvent, GameUiEvent, PendingGameCommands, PendingGameEvents, PendingGameUiEvents,
    QueuedGameCommand,
};
use crate::player::classes::Class;
use crate::player::components::{BaseStats, ChatLog, Player, PlayerIdentity, VitalStats};
use crate::player::progression::{xp_for_level, Experience, PendingXpGrant, PendingXpGrants, LEVEL_CAP};
use crate::player::skills::{grant_level_up_skill_points, SkillSheet};

#[allow(clippy::too_many_arguments)]
pub fn process_admin_progression_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut xp_grants: ResMut<PendingXpGrants>,
    mut player_query: Query<
        (
            &PlayerIdentity,
            &mut Experience,
            &mut SkillSheet,
            &mut BaseStats,
            &mut Class,
            &mut VitalStats,
            &mut ChatLog,
        ),
        With<Player>,
    >,
    mut events: ResMut<PendingGameEvents>,
    mut ui_events: ResMut<PendingGameUiEvents>,
) {
    let queued = std::mem::take(&mut pending_commands.commands);
    let mut remaining = Vec::with_capacity(queued.len());

    for cmd in queued {
        match cmd.command {
            GameCommand::AdminGrantXp { amount } => {
                let Some(target) = cmd.player_id else {
                    continue;
                };
                xp_grants.grants.push(PendingXpGrant {
                    player_id: target,
                    amount,
                });
            }
            GameCommand::AdminSetLevel { level } => {
                let Some(target) = cmd.player_id else {
                    continue;
                };
                let target_level = level.clamp(1, LEVEL_CAP);
                for (identity, mut exp, mut sheet, base_stats, class, _vitals, mut chat) in
                    player_query.iter_mut()
                {
                    if identity.id != target {
                        continue;
                    }
                    let old_level = exp.level;
                    let old_xp = exp.current_xp;
                    let new_xp = xp_for_level(target_level);
                    exp.level = target_level;
                    exp.current_xp = new_xp;

                    if new_xp > old_xp {
                        events.events.push(GameEvent::ExperienceGained {
                            amount: new_xp - old_xp,
                        });
                    } else if new_xp < old_xp {
                        events.events.push(GameEvent::ExperienceLost {
                            amount: old_xp - new_xp,
                        });
                    }
                    if target_level > old_level {
                        for crossed in (old_level + 1)..=target_level {
                            events.events.push(GameEvent::LevelUp { new_level: crossed });
                            ui_events.push(
                                identity.id,
                                GameUiEvent::LevelUpToast { new_level: crossed },
                            );
                            grant_level_up_skill_points(
                                &mut sheet,
                                *class,
                                &base_stats,
                                identity,
                                &mut events,
                                &mut ui_events,
                            );
                        }
                    }
                    chat.push_narrator(format!("[Admin] Level set to {}.", exp.level));
                    break;
                }
            }
            GameCommand::AdminGrantSkillPoints { amount } => {
                let Some(target) = cmd.player_id else {
                    continue;
                };
                for (identity, _exp, mut sheet, _base, _class, _vitals, mut chat) in
                    player_query.iter_mut()
                {
                    if identity.id != target {
                        continue;
                    }
                    sheet.available_points = sheet.available_points.saturating_add(amount);
                    events
                        .events
                        .push(GameEvent::SkillPointsGranted { amount });
                    ui_events
                        .push(identity.id, GameUiEvent::SkillPointsToast { amount });
                    chat.push_narrator(format!("[Admin] Granted {amount} skill points."));
                    break;
                }
            }
            GameCommand::AdminSetSkillRank { skill, rank } => {
                let Some(target) = cmd.player_id else {
                    continue;
                };
                for (identity, _exp, mut sheet, _base, _class, _vitals, mut chat) in
                    player_query.iter_mut()
                {
                    if identity.id != target {
                        continue;
                    }
                    sheet.set_rank(skill, rank);
                    events.events.push(GameEvent::SkillRanksChanged {
                        skill,
                        new_rank: rank,
                        remaining_points: sheet.available_points,
                    });
                    chat.push_narrator(format!(
                        "[Admin] {} rank set to {rank}.",
                        skill.label()
                    ));
                    break;
                }
            }
            GameCommand::AdminSetAttribute { kind, value } => {
                let Some(target) = cmd.player_id else {
                    continue;
                };
                for (identity, _exp, _sheet, mut base, _class, _vitals, mut chat) in
                    player_query.iter_mut()
                {
                    if identity.id != target {
                        continue;
                    }
                    kind.write(&mut base.attributes, value);
                    chat.push_narrator(format!("[Admin] {} set to {value}.", kind.label()));
                    break;
                }
            }
            GameCommand::AdminSetClass { class: new_class } => {
                let Some(target) = cmd.player_id else {
                    continue;
                };
                for (identity, _exp, _sheet, _base, mut class, _vitals, mut chat) in
                    player_query.iter_mut()
                {
                    if identity.id != target {
                        continue;
                    }
                    *class = new_class;
                    events
                        .events
                        .push(GameEvent::PlayerClassChanged { class: new_class });
                    chat.push_narrator(format!("[Admin] Class set to {}.", new_class.label()));
                    break;
                }
            }
            GameCommand::AdminFullHeal => {
                let Some(target) = cmd.player_id else {
                    continue;
                };
                for (identity, _exp, _sheet, _base, _class, mut vitals, mut chat) in
                    player_query.iter_mut()
                {
                    if identity.id != target {
                        continue;
                    }
                    vitals.health = vitals.max_health;
                    vitals.mana = vitals.max_mana;
                    chat.push_narrator("[Admin] Fully healed.".to_owned());
                    break;
                }
            }
            other => remaining.push(QueuedGameCommand {
                player_id: cmd.player_id,
                command: other,
            }),
        }
    }

    pending_commands.commands = remaining;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::{AttributeKind, AttributeSet, PlayerId};
    use crate::player::skills::Skill;

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<PendingGameCommands>()
            .init_resource::<PendingGameEvents>()
            .init_resource::<PendingGameUiEvents>()
            .init_resource::<PendingXpGrants>()
            .add_systems(Update, process_admin_progression_commands);
        app
    }

    fn spawn_player(app: &mut App, id: u64, class: Class) -> Entity {
        let attrs = AttributeSet::new(10, 10, 10, 10, 10, 12);
        let entity = app
            .world_mut()
            .spawn((
                Player,
                PlayerIdentity {
                    id: PlayerId(id),
                    display_name: format!("test#{id}"),
                    home_position: None,
                },
                Experience::default(),
                SkillSheet::default(),
                BaseStats {
                    attributes: attrs,
                    max_health: 0,
                    max_mana: 0,
                    storage_slots: 8,
                },
                class,
                VitalStats {
                    health: 30.0,
                    max_health: 100.0,
                    mana: 5.0,
                    max_mana: 50.0,
                },
                ChatLog::default(),
            ))
            .id();
        entity
    }

    #[test]
    fn admin_grant_xp_routes_through_pending_grants() {
        let mut app = make_app();
        let _entity = spawn_player(&mut app, 1, Class::Fighter);
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(PlayerId(1), GameCommand::AdminGrantXp { amount: 2_500 });

        app.update();

        let grants = app.world().resource::<PendingXpGrants>();
        assert_eq!(grants.grants.len(), 1);
        assert_eq!(grants.grants[0].player_id, PlayerId(1));
        assert_eq!(grants.grants[0].amount, 2_500);
        // Command was drained.
        assert!(app
            .world()
            .resource::<PendingGameCommands>()
            .commands
            .is_empty());
    }

    #[test]
    fn admin_set_level_grants_skill_points_for_each_crossed_level() {
        let mut app = make_app();
        let entity = spawn_player(&mut app, 1, Class::Vagabond); // 8 SP/level base
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(PlayerId(1), GameCommand::AdminSetLevel { level: 4 });

        app.update();

        let exp = app.world().entity(entity).get::<Experience>().unwrap();
        assert_eq!(exp.level, 4);
        assert_eq!(exp.current_xp, xp_for_level(4));

        let sheet = app.world().entity(entity).get::<SkillSheet>().unwrap();
        // Vagabond: base 8 + focus mod (12 → +1) = 9 SP/level. 3 levels crossed = 27.
        assert_eq!(sheet.available_points, 27);
    }

    #[test]
    fn admin_set_skill_rank_bypasses_cap() {
        let mut app = make_app();
        let entity = spawn_player(&mut app, 1, Class::Fighter);
        // Fighter + Thievery is cross-class, cap at L1 = (1+3)/2 = 2.
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::AdminSetSkillRank {
                    skill: Skill::Thievery,
                    rank: 99,
                },
            );

        app.update();

        let sheet = app.world().entity(entity).get::<SkillSheet>().unwrap();
        assert_eq!(sheet.rank(Skill::Thievery), 99);
    }

    #[test]
    fn admin_set_attribute_bypasses_point_buy() {
        let mut app = make_app();
        let entity = spawn_player(&mut app, 1, Class::Fighter);
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::AdminSetAttribute {
                    kind: AttributeKind::Strength,
                    value: 30,
                },
            );

        app.update();

        let base = app.world().entity(entity).get::<BaseStats>().unwrap();
        assert_eq!(base.attributes.strength, 30);
    }

    #[test]
    fn admin_full_heal_caps_at_max() {
        let mut app = make_app();
        let entity = spawn_player(&mut app, 1, Class::Fighter);
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(PlayerId(1), GameCommand::AdminFullHeal);

        app.update();

        let vitals = app.world().entity(entity).get::<VitalStats>().unwrap();
        assert_eq!(vitals.health, vitals.max_health);
        assert_eq!(vitals.mana, vitals.max_mana);
    }

    #[test]
    fn admin_set_class_changes_class_component() {
        let mut app = make_app();
        let entity = spawn_player(&mut app, 1, Class::Fighter);
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::AdminSetClass {
                    class: Class::Vagabond,
                },
            );

        app.update();

        let class = app.world().entity(entity).get::<Class>().unwrap();
        assert_eq!(*class, Class::Vagabond);
    }

    #[test]
    fn admin_grant_skill_points_increments_pool() {
        let mut app = make_app();
        let entity = spawn_player(&mut app, 1, Class::Fighter);
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::AdminGrantSkillPoints { amount: 7 },
            );

        app.update();

        let sheet = app.world().entity(entity).get::<SkillSheet>().unwrap();
        assert_eq!(sheet.available_points, 7);
    }
}
