//! Ambient & social chatter: NPCs with a `Chatter` pool strike up short
//! speech-bubble conversations with nearby idle NPCs, trading lines back and
//! forth. Orthogonal to the combat FSM and the routine overlay — it only acts
//! on NPCs the FSM left in `Wander` with no `CombatTarget`, and it shares the
//! per-NPC bark cooldown (`AiMemory::last_bark_seconds`) with the ambient
//! mutter path so the two never double-fire.
//!
//! Server-authoritative: conversations are tracked in `ConversationRegistry`
//! and emitted as `GameUiEvent::SpeechBubble` (a one-shot signal, not state),
//! mirroring how `update_roaming_npcs` already emits barks.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::combat::components::CombatTarget;
use crate::game::resources::{GameUiEvent, PendingGameUiEvents, SpeechBubbleStyle};
use crate::npc::components::BUBBLE_COOLDOWN_SECONDS;
use crate::npc::components::{AiMemory, AiState, Npc, RoamingRandomState};
use crate::player::components::Player;
use crate::world::components::{
    tile_distance_3d, OverworldObject, SpaceId, SpaceResident, TilePosition,
};
use crate::world::object_definitions::ChatterDef;

/// Seconds between consecutive lines of a conversation (the reply beat).
const REPLY_DELAY_SECONDS: f32 = 2.5;

/// Type-level chatter pool, attached at spawn to NPCs whose definition declares
/// a `chatter:` block.
#[derive(Component, Clone, Debug, Default)]
pub struct Chatter {
    /// One-line openers spoken first when two NPCs meet.
    pub greetings: Vec<String>,
    /// Each topic is an ordered back-and-forth the pair takes turns speaking.
    pub topics: Vec<Vec<String>>,
    /// Max tiles between two NPCs to start (and sustain) a conversation.
    pub radius_tiles: i32,
}

impl Chatter {
    pub fn from_def(def: &ChatterDef) -> Self {
        Self {
            greetings: def.greetings.clone(),
            topics: def.topics.clone(),
            radius_tiles: def.radius_tiles.max(1),
        }
    }
}

/// A live two-NPC conversation. `a` speaks the even-indexed lines (it opened
/// with the greeting), `b` the odd-indexed ones.
#[derive(Clone, Debug)]
struct Conversation {
    a: Entity,
    b: Entity,
    script: Vec<String>,
    next_line: usize,
    /// Elapsed-seconds deadline for the next line.
    speak_at: f32,
    /// Drift tolerance: reaped once the pair separates beyond this.
    radius: i32,
}

/// All in-flight conversations, keyed by the sorted entity pair.
#[derive(Resource, Default)]
pub struct ConversationRegistry {
    active: HashMap<(Entity, Entity), Conversation>,
}

impl ConversationRegistry {
    /// Whether `entity` is currently a participant in a conversation. Used by
    /// the NPC debug overlay to surface social state.
    pub fn is_conversing(&self, entity: Entity) -> bool {
        self.active
            .keys()
            .any(|(a, b)| *a == entity || *b == entity)
    }
}

/// Per-NPC snapshot taken once per tick (read-only pass) so the pairing and
/// reaping logic can see every chattering NPC's position without holding a
/// mutable query borrow.
#[derive(Clone, Copy)]
struct ChatterInfo {
    space: SpaceId,
    tile: TilePosition,
    object_id: Option<u64>,
    radius: i32,
    in_combat: bool,
    /// `Wander`, not in combat, and off the bark cooldown — may *start* a chat.
    eligible: bool,
}

fn pair_key(a: Entity, b: Entity) -> (Entity, Entity) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Drives ambient/social conversations: reaps stale ones, advances active ones
/// a line at a time, and pairs up fresh eligible NPCs. Runs after
/// `update_roaming_npcs` so it sees post-step positions and final `AiState`.
pub fn tick_social_chatter(
    time: Res<Time>,
    mut registry: ResMut<ConversationRegistry>,
    mut ui_events: Option<ResMut<PendingGameUiEvents>>,
    mut npc_query: Query<
        (
            Entity,
            &SpaceResident,
            &TilePosition,
            &Chatter,
            &AiState,
            &mut AiMemory,
            &mut RoamingRandomState,
            Option<&OverworldObject>,
            Has<CombatTarget>,
        ),
        (With<Npc>, Without<Player>),
    >,
) {
    let Some(ui_events) = ui_events.as_deref_mut() else {
        return;
    };
    let elapsed = time.elapsed_secs();

    // 1. Read-only snapshot of every chattering NPC.
    let info: HashMap<Entity, ChatterInfo> = npc_query
        .iter()
        .map(
            |(entity, resident, tile, chatter, ai, memory, _rng, object, in_combat)| {
                let eligible = matches!(ai, AiState::Wander)
                    && !in_combat
                    && elapsed - memory.last_bark_seconds >= BUBBLE_COOLDOWN_SECONDS;
                (
                    entity,
                    ChatterInfo {
                        space: resident.space_id,
                        tile: *tile,
                        object_id: object.map(|o| o.object_id),
                        radius: chatter.radius_tiles,
                        in_combat,
                        eligible,
                    },
                )
            },
        )
        .collect();

    // 2. Reap conversations whose participants vanished, entered combat, or
    //    drifted apart.
    registry
        .active
        .retain(|_, conv| match (info.get(&conv.a), info.get(&conv.b)) {
            (Some(a), Some(b)) => {
                a.space == b.space
                    && a.tile.z == b.tile.z
                    && !a.in_combat
                    && !b.in_combat
                    && tile_distance_3d(a.tile, b.tile) <= conv.radius + 1
            }
            _ => false,
        });

    // 3. Advance active conversations one line at a time.
    let mut finished: Vec<(Entity, Entity)> = Vec::new();
    for (key, conv) in registry.active.iter_mut() {
        if elapsed < conv.speak_at {
            continue;
        }
        if conv.next_line >= conv.script.len() {
            finished.push(*key);
            continue;
        }
        let speaker = if conv.next_line % 2 == 0 {
            conv.a
        } else {
            conv.b
        };
        if let Some(object_id) = info.get(&speaker).and_then(|i| i.object_id) {
            ui_events.push_broadcast(GameUiEvent::SpeechBubble {
                speaker_object_id: object_id,
                text: conv.script[conv.next_line].clone(),
                style: SpeechBubbleStyle::Say,
            });
        }
        if let Ok((.., mut memory, _, _, _)) = npc_query.get_mut(speaker) {
            memory.last_bark_seconds = elapsed;
        }
        conv.next_line += 1;
        conv.speak_at = elapsed + REPLY_DELAY_SECONDS;
        if conv.next_line >= conv.script.len() {
            finished.push(*key);
        }
    }
    for key in finished {
        registry.active.remove(&key);
    }

    // 4. Pair up fresh eligible NPCs that aren't already conversing.
    let mut used: HashSet<Entity> = registry.active.keys().flat_map(|(a, b)| [*a, *b]).collect();
    let eligible: Vec<Entity> = info
        .iter()
        .filter(|(e, i)| i.eligible && !used.contains(e))
        .map(|(e, _)| *e)
        .collect();

    for i in 0..eligible.len() {
        let a = eligible[i];
        if used.contains(&a) {
            continue;
        }
        let ia = info[&a];
        for &b in eligible.iter().skip(i + 1) {
            if used.contains(&b) {
                continue;
            }
            let ib = info[&b];
            if ia.space != ib.space || ia.tile.z != ib.tile.z {
                continue;
            }
            let radius = ia.radius.min(ib.radius);
            if tile_distance_3d(ia.tile, ib.tile) > radius {
                continue;
            }
            // `a` opens; build its script from its own chatter pool + RNG.
            let Ok((_, _, _, chatter, _, _, mut rng, _, _)) = npc_query.get_mut(a) else {
                continue;
            };
            let script = build_script(chatter, &mut rng);
            if script.is_empty() {
                continue;
            }
            registry.active.insert(
                pair_key(a, b),
                Conversation {
                    a,
                    b,
                    script,
                    next_line: 0,
                    speak_at: elapsed,
                    radius,
                },
            );
            used.insert(a);
            used.insert(b);
            break;
        }
    }
}

/// Build a conversation script: an optional greeting opener followed by one
/// randomly-chosen topic's back-and-forth lines.
fn build_script(chatter: &Chatter, rng: &mut RoamingRandomState) -> Vec<String> {
    let mut script = Vec::new();
    if !chatter.greetings.is_empty() {
        script.push(chatter.greetings[rng.next_index(chatter.greetings.len())].clone());
    }
    if !chatter.topics.is_empty() {
        let topic = &chatter.topics[rng.next_index(chatter.topics.len())];
        script.extend(topic.iter().cloned());
    }
    script
}
