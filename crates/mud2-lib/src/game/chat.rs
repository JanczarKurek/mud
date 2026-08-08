use bevy::prelude::*;

use crate::combat::systems::chebyshev_distance;
use crate::game::commands::GameCommand;
use crate::game::helpers::resolve_acting_player;
use crate::game::resources::{
    ChatLogState, GameUiEvent, PendingGameCommands, PendingGameUiEvents, SpeechBubbleStyle,
};
use crate::player::components::{Player, PlayerIdentity};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};

pub const CHAT_RADIUS_TILES: i32 = 10;
pub const CHAT_MAX_LEN: usize = 200;

pub fn process_say_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut player_query: Query<
        (
            &PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            &mut ChatLogState,
        ),
        With<Player>,
    >,
) {
    for (queued_player_id, text) in pending_commands.drain_matching(|command| match command {
        GameCommand::Say { text } => Ok(text),
        other => Err(other),
    }) {
        let speaker = resolve_acting_player(player_query.iter(), queued_player_id, |row| row.0.id);
        let Some((speaker_identity, speaker_space, speaker_tile, speaker_object, _)) = speaker
        else {
            continue;
        };
        let speaker_id = speaker_identity.id;
        let speaker_space_id = speaker_space.space_id;
        let speaker_tile = *speaker_tile;
        let speaker_name = speaker_identity.display_name.clone();
        let speaker_object_id = speaker_object.object_id;

        let trimmed = text.trim();
        if trimmed.is_empty() {
            push_narrator_to(&mut player_query, speaker_id, "Empty message.");
            continue;
        }
        if trimmed.chars().count() > CHAT_MAX_LEN {
            push_narrator_to(
                &mut player_query,
                speaker_id,
                format!("Message too long (max {CHAT_MAX_LEN} characters)."),
            );
            continue;
        }

        let line = format!("[{speaker_name}]: {trimmed}");
        for (_, resident, tile, _, mut chat_log) in player_query.iter_mut() {
            if resident.space_id != speaker_space_id {
                continue;
            }
            if chebyshev_distance(&speaker_tile, tile) > CHAT_RADIUS_TILES {
                continue;
            }
            chat_log.push_line(line.clone());
        }

        // Also fire a floating bubble. The bubble overlay can't render
        // non-ASCII glyphs with the default font, so strip any non-ASCII
        // chars (the chat log itself keeps the original text).
        let bubble_text = sanitize_for_bubble(trimmed);
        if !bubble_text.is_empty() {
            ui_events.push_broadcast(GameUiEvent::SpeechBubble {
                speaker_object_id,
                text: bubble_text,
                style: SpeechBubbleStyle::Say,
            });
        }
    }
}

/// Drop characters the bubble font can't render. The default Bevy font in
/// this project only ships ASCII glyphs; anything else renders as tofu
/// boxes, so the bubble path filters server-side.
pub(crate) fn sanitize_for_bubble(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii() && (!c.is_ascii_control() || *c == ' '))
        .collect::<String>()
        .trim()
        .to_owned()
}

fn push_narrator_to(
    player_query: &mut Query<
        (
            &PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            &mut ChatLogState,
        ),
        With<Player>,
    >,
    player_id: crate::player::components::PlayerId,
    message: impl Into<String>,
) {
    if let Some((_, _, _, _, mut chat_log)) = player_query
        .iter_mut()
        .find(|(identity, _, _, _, _)| identity.id == player_id)
    {
        chat_log.push_narrator(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::resources::{PendingGameCommands, PendingGameUiEvents};
    use crate::player::components::{ChatLog, PlayerId};
    use crate::world::components::SpaceId;

    fn spawn_player(
        app: &mut App,
        player_id: u64,
        display_name: &str,
        space_id: SpaceId,
        tile: TilePosition,
    ) -> Entity {
        app.world_mut()
            .spawn((
                Player,
                PlayerIdentity::with_display_name(PlayerId(player_id), display_name.to_owned()),
                ChatLog::default(),
                SpaceResident { space_id },
                tile,
                OverworldObject {
                    object_id: 1000 + player_id,
                    definition_id: "player".to_owned(),
                    placement_seq: 0,
                },
            ))
            .id()
    }

    fn build_app() -> App {
        let mut app = crate::test_support::minimal_command_app();
        app.add_systems(Update, process_say_commands);
        app
    }

    fn chat_lines(app: &App, entity: Entity) -> Vec<String> {
        app.world()
            .entity(entity)
            .get::<ChatLog>()
            .unwrap()
            .lines
            .clone()
    }

    #[test]
    fn say_broadcasts_to_nearby_players_in_same_space() {
        let mut app = build_app();
        let space = SpaceId(0);
        let speaker = spawn_player(&mut app, 1, "alice", space, TilePosition::ground(5, 5));
        let near = spawn_player(&mut app, 2, "bob", space, TilePosition::ground(8, 6));
        let far = spawn_player(&mut app, 3, "carol", space, TilePosition::ground(50, 50));
        let other_space = spawn_player(&mut app, 4, "dave", SpaceId(1), TilePosition::ground(5, 5));

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::Say {
                    text: "hello".to_owned(),
                },
            );
        app.update();

        assert!(chat_lines(&app, speaker)
            .iter()
            .any(|l| l == "[alice]: hello"));
        assert!(chat_lines(&app, near).iter().any(|l| l == "[alice]: hello"));
        assert!(chat_lines(&app, far).iter().all(|l| l != "[alice]: hello"));
        assert!(chat_lines(&app, other_space)
            .iter()
            .all(|l| l != "[alice]: hello"));
    }

    #[test]
    fn empty_message_only_warns_the_speaker() {
        let mut app = build_app();
        let speaker = spawn_player(&mut app, 1, "alice", SpaceId(0), TilePosition::ground(0, 0));
        let bystander = spawn_player(&mut app, 2, "bob", SpaceId(0), TilePosition::ground(0, 1));

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::Say {
                    text: "   ".to_owned(),
                },
            );
        app.update();

        let speaker_chat = chat_lines(&app, speaker);
        assert!(speaker_chat.iter().any(|l| l.contains("Empty message")));
        let bystander_chat = chat_lines(&app, bystander);
        assert!(bystander_chat.iter().all(|l| !l.contains("Empty message")));
    }

    #[test]
    fn over_length_message_is_rejected() {
        let mut app = build_app();
        let speaker = spawn_player(&mut app, 1, "alice", SpaceId(0), TilePosition::ground(0, 0));

        let text: String = "x".repeat(CHAT_MAX_LEN + 1);
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(PlayerId(1), GameCommand::Say { text });
        app.update();

        let speaker_chat = chat_lines(&app, speaker);
        assert!(speaker_chat.iter().any(|l| l.contains("Message too long")));
    }

    #[test]
    fn say_emits_speech_bubble_event() {
        let mut app = build_app();
        spawn_player(&mut app, 1, "alice", SpaceId(0), TilePosition::ground(0, 0));

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::Say {
                    text: "hi there".to_owned(),
                },
            );
        app.update();

        let events = &app.world().resource::<PendingGameUiEvents>().broadcast;
        let bubble = events
            .iter()
            .find_map(|ev| match ev {
                GameUiEvent::SpeechBubble {
                    speaker_object_id,
                    text,
                    style,
                } => Some((*speaker_object_id, text.clone(), *style)),
                _ => None,
            })
            .expect("expected a SpeechBubble event for the Say command");
        assert_eq!(bubble.0, 1001);
        assert_eq!(bubble.1, "hi there");
        assert_eq!(bubble.2, SpeechBubbleStyle::Say);
    }

    #[test]
    fn say_bubble_strips_non_ascii_glyphs() {
        assert_eq!(sanitize_for_bubble("hello \u{1F600} world"), "hello  world");
        assert_eq!(sanitize_for_bubble("\u{1F480}"), "");
        assert_eq!(sanitize_for_bubble("plain"), "plain");
    }
}
