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
    for (queued_player_id, command) in pending_commands.drain_matching(|command| match command {
        claimed @ (GameCommand::UpsertLogEntry { .. }
        | GameCommand::DeleteLogEntry { .. }
        | GameCommand::SetQuestPlayerNotes { .. }
        | GameCommand::SetLogPlayerNotes { .. }) => Ok(claimed),
        other => Err(other),
    }) {
        let acting =
            queued_player_id.or_else(|| players.iter().next().map(|(identity, _)| identity.id));
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
        let mutated = apply_command(&command, &mut log);
        if mutated {
            log.write_to_stash(&mut stash);
        }
    }
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
        // Quests-section shorthand — same path as `SetLogPlayerNotes`.
        GameCommand::SetQuestPlayerNotes { quest_name, text } => {
            set_player_notes(log, QUESTS_SECTION, quest_name, text)
        }
        GameCommand::SetLogPlayerNotes {
            section,
            subsection,
            text,
        } => set_player_notes(log, section, subsection, text),
        _ => false,
    }
}

/// Write `text` into the `player_notes` field of one entry, in any section.
/// The engine-owned `body` is never touched, so this is the one write a player
/// is allowed to make against an engine entry (quest, dossier, or bestiary).
fn set_player_notes(log: &mut LogState, section: &str, subsection: &str, text: &str) -> bool {
    if text.chars().count() > MAX_PLAYER_NOTES_LEN {
        warn!("SetLogPlayerNotes rejected: player_notes too long");
        return false;
    }
    let Some(entry) = log.entry_mut(section, subsection) else {
        warn!("SetLogPlayerNotes rejected: entry {section}/{subsection} does not exist");
        return false;
    };
    if entry.player_notes == *text {
        return false;
    }
    entry.player_notes = text.to_owned();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{LogEntry, LogOwner, BESTIARY_SECTION, NOTES_SECTION, PEOPLE_SECTION};

    fn engine_entry(log: &mut LogState, section: &str, subsection: &str) {
        log.upsert(
            section.to_owned(),
            subsection.to_owned(),
            LogEntry {
                title: "t".to_owned(),
                body: "b".to_owned(),
                player_notes: String::new(),
                owner: LogOwner::Engine,
            },
        );
    }

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

    /// Pointed at the Bestiary rather than Quests to prove the owner guard is
    /// section-agnostic — codex entries must be as read-only as quest entries.
    #[test]
    fn player_cannot_overwrite_engine_entry() {
        let mut log = LogState::default();
        log.upsert(
            BESTIARY_SECTION.to_owned(),
            "wolf".to_owned(),
            LogEntry {
                title: "engine title".to_owned(),
                body: "engine body".to_owned(),
                player_notes: String::new(),
                owner: LogOwner::Engine,
            },
        );
        let mutated = apply_command(
            &upsert_player(BESTIARY_SECTION, "wolf", "hax", "hax"),
            &mut log,
        );
        assert!(!mutated);
        assert_eq!(
            log.entry(BESTIARY_SECTION, "wolf").unwrap().body,
            "engine body"
        );
    }

    #[test]
    fn player_cannot_delete_engine_entry() {
        let mut log = LogState::default();
        engine_entry(&mut log, BESTIARY_SECTION, "wolf");
        let mutated = apply_command(
            &GameCommand::DeleteLogEntry {
                section: BESTIARY_SECTION.to_owned(),
                subsection: "wolf".to_owned(),
            },
            &mut log,
        );
        assert!(!mutated);
        assert!(log.entry(BESTIARY_SECTION, "wolf").is_some());
    }

    #[test]
    fn set_log_player_notes_works_on_any_section() {
        let mut log = LogState::default();
        engine_entry(&mut log, PEOPLE_SECTION, "villager");
        engine_entry(&mut log, BESTIARY_SECTION, "wolf");

        for (section, subsection) in [(PEOPLE_SECTION, "villager"), (BESTIARY_SECTION, "wolf")] {
            let mutated = apply_command(
                &GameCommand::SetLogPlayerNotes {
                    section: section.to_owned(),
                    subsection: subsection.to_owned(),
                    text: "sells apples cheap".to_owned(),
                },
                &mut log,
            );
            assert!(mutated, "{section} notes should be writable");
            let entry = log.entry(section, subsection).unwrap();
            assert_eq!(entry.player_notes, "sells apples cheap");
            // The engine body is never touched by a notes write.
            assert_eq!(entry.body, "b");
        }
    }

    #[test]
    fn set_log_player_notes_requires_existing_entry() {
        let mut log = LogState::default();
        let mutated = apply_command(
            &GameCommand::SetLogPlayerNotes {
                section: PEOPLE_SECTION.to_owned(),
                subsection: "nobody".to_owned(),
                text: "hi".to_owned(),
            },
            &mut log,
        );
        assert!(!mutated);
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
