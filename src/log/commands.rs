//! Server-side handlers for log commands. Drains `UpsertLogEntry`,
//! `DeleteLogEntry`, and `SetQuestPlayerNotes` from `PendingGameCommands` and
//! mutates the acting player's `CharacterStash["log"]` after enforcing length
//! caps and owner gating.

use bevy::prelude::*;

use crate::crafting::CharacterStash;
use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::log::{
    LogEntry, LogOwner, LogState, MAX_BODY_LEN, MAX_PLAYER_NOTES_LEN, MAX_SECTIONS_PER_PLAYER,
    MAX_SECTION_KEY_LEN, MAX_SUBENTRIES_PER_SECTION, MAX_SUBSECTION_KEY_LEN, MAX_TITLE_LEN,
    QUESTS_SECTION,
};
use crate::player::components::{Player, PlayerId, PlayerIdentity};

/// Drains log commands from `PendingGameCommands`. Mirrors the structure of
/// `process_stash_commands` in `src/crafting/systems.rs`: filter out the
/// variants this system handles, dispatch them, and put the rest back.
pub fn process_log_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut players: Query<(&PlayerIdentity, &mut CharacterStash), With<Player>>,
) {
    if pending_commands.commands.is_empty() {
        return;
    }
    let drained: Vec<_> = std::mem::take(&mut pending_commands.commands);
    let mut remaining = Vec::with_capacity(drained.len());

    for queued in drained {
        let is_log = matches!(
            queued.command,
            GameCommand::UpsertLogEntry { .. }
                | GameCommand::DeleteLogEntry { .. }
                | GameCommand::SetQuestPlayerNotes { .. }
        );
        if !is_log {
            remaining.push(queued);
            continue;
        }

        let acting = queued
            .player_id
            .or_else(|| players.iter().next().map(|(identity, _)| identity.id));
        let Some(PlayerId(target_id)) = acting else {
            continue;
        };

        let Some((_, mut stash)) = players
            .iter_mut()
            .find(|(identity, _)| identity.id.0 == target_id)
        else {
            warn!("log command dropped: no player entity for id {target_id}");
            continue;
        };

        let mut log = LogState::from_stash(&stash);
        let mutated = apply_command(&queued.command, &mut log);
        if mutated {
            log.write_to_stash(&mut stash);
        }
    }

    pending_commands.commands = remaining;
}

/// Returns `true` when the command mutated `log` (so the caller should flush
/// back to stash). Pure on `log` — no Bevy access — to keep it unit-testable.
fn apply_command(command: &GameCommand, log: &mut LogState) -> bool {
    match command {
        GameCommand::UpsertLogEntry {
            section,
            subsection,
            title,
            body,
            owner,
        } => {
            let section = section.trim();
            let subsection = subsection.trim();
            if section.is_empty() || subsection.is_empty() {
                return false;
            }
            if section.chars().count() > MAX_SECTION_KEY_LEN
                || subsection.chars().count() > MAX_SUBSECTION_KEY_LEN
            {
                warn!("UpsertLogEntry rejected: section/subsection key too long");
                return false;
            }
            if title.chars().count() > MAX_TITLE_LEN {
                warn!("UpsertLogEntry rejected: title too long");
                return false;
            }
            if body.chars().count() > MAX_BODY_LEN {
                warn!("UpsertLogEntry rejected: body too long");
                return false;
            }

            // Existence + owner gating
            let existing_owner = log.entry(section, subsection).map(|e| e.owner);
            let existing_player_notes = log
                .entry(section, subsection)
                .map(|e| e.player_notes.clone())
                .unwrap_or_default();

            // If the entry exists and is engine-owned, a Player-issued
            // upsert is rejected. Engine writes always win — they replace
            // title/body but preserve any player_notes the player added.
            let is_engine_write = matches!(owner, LogOwner::Engine);
            if let Some(LogOwner::Engine) = existing_owner {
                if !is_engine_write {
                    warn!("UpsertLogEntry rejected: cannot overwrite engine-owned entry");
                    return false;
                }
            }

            // Growth caps when adding a *new* entry.
            if existing_owner.is_none() {
                if log.sections.len() >= MAX_SECTIONS_PER_PLAYER
                    && !log.sections.contains_key(section)
                {
                    warn!("UpsertLogEntry rejected: too many sections");
                    return false;
                }
                if let Some(s) = log.section(section) {
                    if s.subsections.len() >= MAX_SUBENTRIES_PER_SECTION {
                        warn!("UpsertLogEntry rejected: too many subentries in section");
                        return false;
                    }
                }
            }

            let entry = LogEntry {
                title: title.clone(),
                body: body.clone(),
                player_notes: existing_player_notes,
                owner: *owner,
            };
            log.upsert(section.to_owned(), subsection.to_owned(), entry);
            true
        }
        GameCommand::DeleteLogEntry {
            section,
            subsection,
        } => {
            let Some(entry) = log.entry(section, subsection) else {
                return false;
            };
            if matches!(entry.owner, LogOwner::Engine) {
                warn!("DeleteLogEntry rejected: cannot delete engine-owned entry");
                return false;
            }
            log.remove(section, subsection).is_some()
        }
        GameCommand::SetQuestPlayerNotes { quest_name, text } => {
            if text.chars().count() > MAX_PLAYER_NOTES_LEN {
                warn!("SetQuestPlayerNotes rejected: player_notes too long");
                return false;
            }
            let Some(entry) = log.entry_mut(QUESTS_SECTION, quest_name) else {
                warn!("SetQuestPlayerNotes rejected: quest entry does not exist");
                return false;
            };
            if entry.player_notes == *text {
                return false;
            }
            entry.player_notes = text.clone();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{LogEntry, LogOwner, NOTES_SECTION};

    fn upsert_player(section: &str, subsection: &str, title: &str, body: &str) -> GameCommand {
        GameCommand::UpsertLogEntry {
            section: section.to_owned(),
            subsection: subsection.to_owned(),
            title: title.to_owned(),
            body: body.to_owned(),
            owner: LogOwner::Player,
        }
    }

    #[test]
    fn player_upsert_creates_entry() {
        let mut log = LogState::default();
        assert!(apply_command(
            &upsert_player(NOTES_SECTION, "n1", "title", "body"),
            &mut log,
        ));
        let entry = log.entry(NOTES_SECTION, "n1").unwrap();
        assert_eq!(entry.title, "title");
        assert_eq!(entry.body, "body");
        assert_eq!(entry.owner, LogOwner::Player);
    }

    #[test]
    fn engine_upsert_preserves_player_notes() {
        let mut log = LogState::default();
        log.upsert(
            QUESTS_SECTION.to_owned(),
            "demo".to_owned(),
            LogEntry {
                title: "old title".to_owned(),
                body: "old body".to_owned(),
                player_notes: "scratchpad".to_owned(),
                owner: LogOwner::Engine,
            },
        );
        let mutated = apply_command(
            &GameCommand::UpsertLogEntry {
                section: QUESTS_SECTION.to_owned(),
                subsection: "demo".to_owned(),
                title: "new title".to_owned(),
                body: "new body".to_owned(),
                owner: LogOwner::Engine,
            },
            &mut log,
        );
        assert!(mutated);
        let entry = log.entry(QUESTS_SECTION, "demo").unwrap();
        assert_eq!(entry.title, "new title");
        assert_eq!(entry.body, "new body");
        assert_eq!(entry.player_notes, "scratchpad");
    }

    #[test]
    fn player_cannot_overwrite_engine_entry() {
        let mut log = LogState::default();
        log.upsert(
            QUESTS_SECTION.to_owned(),
            "demo".to_owned(),
            LogEntry {
                title: "engine title".to_owned(),
                body: "engine body".to_owned(),
                player_notes: String::new(),
                owner: LogOwner::Engine,
            },
        );
        let mutated = apply_command(
            &upsert_player(QUESTS_SECTION, "demo", "hax", "hax"),
            &mut log,
        );
        assert!(!mutated);
        assert_eq!(
            log.entry(QUESTS_SECTION, "demo").unwrap().body,
            "engine body"
        );
    }

    #[test]
    fn player_cannot_delete_engine_entry() {
        let mut log = LogState::default();
        log.upsert(
            QUESTS_SECTION.to_owned(),
            "demo".to_owned(),
            LogEntry {
                title: "t".to_owned(),
                body: "b".to_owned(),
                player_notes: String::new(),
                owner: LogOwner::Engine,
            },
        );
        let mutated = apply_command(
            &GameCommand::DeleteLogEntry {
                section: QUESTS_SECTION.to_owned(),
                subsection: "demo".to_owned(),
            },
            &mut log,
        );
        assert!(!mutated);
        assert!(log.entry(QUESTS_SECTION, "demo").is_some());
    }

    #[test]
    fn set_quest_player_notes_requires_existing_quest() {
        let mut log = LogState::default();
        let mutated = apply_command(
            &GameCommand::SetQuestPlayerNotes {
                quest_name: "ghost".to_owned(),
                text: "hi".to_owned(),
            },
            &mut log,
        );
        assert!(!mutated);
    }

    #[test]
    fn set_quest_player_notes_writes_only_player_notes() {
        let mut log = LogState::default();
        log.upsert(
            QUESTS_SECTION.to_owned(),
            "demo".to_owned(),
            LogEntry {
                title: "t".to_owned(),
                body: "b".to_owned(),
                player_notes: String::new(),
                owner: LogOwner::Engine,
            },
        );
        let mutated = apply_command(
            &GameCommand::SetQuestPlayerNotes {
                quest_name: "demo".to_owned(),
                text: "my scribbles".to_owned(),
            },
            &mut log,
        );
        assert!(mutated);
        let entry = log.entry(QUESTS_SECTION, "demo").unwrap();
        assert_eq!(entry.body, "b");
        assert_eq!(entry.player_notes, "my scribbles");
    }

    #[test]
    fn length_caps_reject_oversize_body() {
        let mut log = LogState::default();
        let big = "x".repeat(MAX_BODY_LEN + 1);
        let mutated = apply_command(&upsert_player(NOTES_SECTION, "n1", "title", &big), &mut log);
        assert!(!mutated);
        assert!(log.entry(NOTES_SECTION, "n1").is_none());
    }
}
