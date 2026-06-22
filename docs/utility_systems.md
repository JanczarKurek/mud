# Utility & Simulation Systems

Design reference for Mud 2.0's **non-combat depth**: the systems that make the
game an interaction-driven *simulator* rather than a fighting game. This doc is
the source of truth for the design; `PLAN.md` §4.2 and `docs/progression.md` §5
point here.

The companion docs split responsibility like this:
- `docs/progression.md` — *who the character is*: classes, XP/levels, the skill
  list and per-skill mechanic, combat math, death penalty.
- **this doc** — *how the character interacts with the world*: the shared world
  signals and the four interlocking utility clusters built on top of them.

Tuning numbers are marked **`[tunable]`** and gathered in §7.

---

## 1. Goals & non-goals

### Goals
- Make the world **legible and manipulable**: light, noise, terrain, and objects
  are things the player reads and acts on, not just scenery.
- Turn isolated skill checks into **systems that feed each other** — the central
  thesis of §2.
- Reward deliberate, low-combat play: sneaking, hauling, gathering, crafting,
  identifying, recovering. Pacing comes from interaction and downtime, not DPS.
- Give every utility skill a job that *matters because another system reads its
  output*.

### Non-goals (deferred)
- Punishing survival sim. Sim depth is **Medium** (§6): fatigue + rest + optional
  forage, but **no** hunger/thirst/temperature/weather attrition.
- A full physics/voxel push model. "Moving stuff" is grid-tile object relocation
  with gameplay consequences, not rigid-body simulation.
- Reworking combat. Combat stays as designed in `docs/progression.md` §7; this
  doc only *connects* to it (noise, sneak-attack, monster-weakness lore).

---

## 2. Core thesis — shared world signals

Today every skill is a **private gate**: pick a lock, climb a ledge, hide an
object — each check resolves in isolation and changes nothing else. That makes a
list of features, not a simulation.

The fix is a small set of **ambient signal channels** that many systems both
*produce* and *consume*. These shared currencies are what make the systems
"intertwined": a choice in one (light a torch, drop a crate, sprint past a guard)
ripples through the others.

| Signal | What it is | Produced by | Consumed by |
|---|---|---|---|
| **Light level** | `light_level_at(tile) → 0.0..1.0` from the day/night clock + nearby emitters | `WorldClock`, light-emitting objects, carried torches, the Glimmer effect | Stealth detection (§3), foraging (§5), ambience |
| **Noise** | a short-lived, decaying field of `NoiseEvent { tile, loudness }` | movement (loud/quiet by sneak), doors, force-lock, mining, combat, crafting | NPC hearing → investigate (§3) |
| **Cover / line-of-sight** | the existing `los_blockers` index + movable objects | walls, placed crates/barrels | Stealth detection (§3), ranged combat |
| **Exertion** | a per-player fatigue meter (the Medium-sim currency, §6) | climb/jump/push, sneaking, combat | regen rate, skill-check penalty, forced downtime; recovered by rest/food; governed by Endurance |
| **Success margin** | `check.total − dc` (already returned by `skill_check`) | every skill check | gather yield, craft/enchant **quality tier** (`ItemModifier`), lock/hide quality |

Design rule: **a new utility mechanic should read or write at least one shared
signal.** If it can't, it's probably another private gate and should be
reconsidered. The four clusters below are organized around which signals they
touch.

### 2.1 Existing substrate (don't rebuild)

Much of the surface already exists and is reused rather than replaced:

- Skills + `skill_check(sheet, attrs, skill, dc, situational) → {roll,total,success}` — `src/player/skills.rs`.
- Acrobatics: Athletics gates climb, jump-by-DC, fall-damage saves — `src/game/traversal.rs`, `src/game/systems.rs`.
- Hiding objects: `can_hide` → `Hidden`, Thievery to hide / passive Perception to spot — `src/world/hide_action.rs`, `src/world/hidden.rs`.
- Crafting: recipes (inputs/outputs/station/auto-learn/xp), `CraftItem` — `src/crafting/`.
- Gather nodes: tool-gate + skill-gate + respawn + `grants_items` — e.g. `assets/overworld_objects/{herb_patch,ore_node}`.
- Status effects with sophisticated stacking — `src/magic/effects.rs`.
- Day/night clock + dynamic light emission — `src/world/lighting.rs`.
- Movable/draggable objects, containers, weight/encumbrance, object state machines + wiring — `src/world/`.
- Per-instance `ItemModifier` enchantments (data exists; no apply-UX yet) — `src/player/components.rs`.

---

## 3. Cluster A — Stealth & Detection web

*The "hiding" showcase. Signals: light, noise, cover.* This is the first
implementation slice (§8) because Stealth is currently wired to **nothing** —
NPC detection is a fixed radius that ignores the player entirely.

- **Sneak mode** (toggle): slower movement, much quieter (§ noise), and the
  player now rolls Stealth against observers.
- **Opposed detection.** NPC detection becomes an opposed roll instead of a fixed
  radius: `npc = d20 + perception + light_bonus` vs
  `player = d20 + stealth_total + sneak_bonus`. Modifiers come from the shared
  signals — **light level** at the target tile, whether the player is **sneaking**,
  **cover** (the existing binary LoS gate stays as hard cover), and **recent
  noise**. The hard sensing radius remains as a cheap early-out.
- **Light tradeoff.** Carrying a lit torch (a light emitter) makes you trivially
  visible — sneaking is a night/shadow playstyle and interacts directly with the
  day/night clock.
- **Search behavior.** An NPC that hears noise or loses sight transitions to the
  existing `AiState::Alert`, walking toward the last-known / noise tile, and must
  re-acquire by Perception. No new AI states needed.
- **Sneak attack.** An attack from undetected stealth grants a damage bonus and is
  the trigger for the Vagabond **Backstab** class feature
  (`docs/progression.md` §3.4). *Net-new and cross-boundary — deferred (§8).*

Cross-links: lighting + light items, LoS + movable cover (Cluster B), noise
(Cluster B + combat), Perception (also spots hidden objects/traps), Backstab
(class).

---

## 4. Cluster B — Physical world manipulation

*The "moving stuff" showcase. Signals: cover, noise, exertion, success margin.*
Movable objects already exist as drag/relocate; this cluster makes relocation
*matter*.

- **Push/pull heavy objects.** Athletics check vs the object's `weight`; costs
  Exertion (§6). Light objects move freely.
- **Objects as cover.** A crate/barrel placed between the player and an NPC
  contributes to the cover/LoS calc in §3 — you can build a sneaking route.
- **Stack to climb.** Push a crate against a ledge to enable or ease the existing
  Athletics climb (reach otherwise-unreachable tiles / z-levels).
- **Barricade.** Place a colliding object to block NPC A* pathing — buys time and
  reroutes pursuers.
- **Pressure plates / triggers.** Hold down the existing `on_stepped` + wired
  triggers with a heavy object (a puzzle layer on top of the lever/door wiring
  that already exists).

Cross-links: Athletics (push/climb), Exertion (cost), Stealth (cover), NPC
pathfinding (barricade), wiring/plates (existing), noise (pushing is loud).

---

## 5. Cluster C — Production & quality chains

*The "crafting" showcase. Signals: success margin, light (forage).* Crafting
exists but is binary (fixed inputs → fixed output). This cluster adds depth via
**success margin → quality**.

- **Gather quality.** Survival check **margin** scales yield (replacing fixed
  `uniform(1,3)`) and rolls a rare-reagent chance on a high margin.
- **Craft quality.** A recipe may carry an optional `skill: { skill, dc }`. The
  margin maps to an output **quality tier**, applied as an `ItemModifier` (infra
  exists) — e.g. *fine* (+1), *masterwork* (+2). Failure wastes some inputs (the
  Medium-sim consequence).
- **Spellcraft = magic utility** (per `docs/progression.md` §5, *not* spell
  damage):
  - **Enchant:** base item + reagent + Spellcraft check → apply a magical
    `ItemModifier`.
  - **Learn from scroll:** Spellcraft check identifies and learns a spell from a
    scroll, feeding the Wizard **Spellbook** feature.
- **Economy tie.** Item quality (modifiers) raises the Lore appraisal base price
  and the Persuasion haggle (`src/game/trade.rs`). A master crafter feeds the
  economy.

Cross-links: Survival (gather), crafting system, `ItemModifier` (quality),
Spellcraft (enchant/learn), Lore + Persuasion (economy), Focus.

---

## 6. Cluster D — Knowledge & body loop (incl. Medium-sim Exertion)

*Signals: success margin, exertion, light.* Makes information and recovery
skill-gated, and introduces the Medium-sim fatigue currency.

- **Lore identify.** Items can spawn `identified: false` (especially modified
  ones); inspecting shows only base stats until a Lore check reveals
  stats/value. Monster Lore reveals weaknesses/resistances (ties to the
  element/status system). Reuses the existing `inspect_range`.
- **Spellcraft identify.** Reveal magical auras — which enchant an item carries,
  what an unknown potion does.
- **Heal.** A **bandage** action restores HP out of combat (margin → amount), a
  **cure-status** action removes Poisoned / Burning / Chill from the existing
  `MagicEffects`, and Heal multiplies potion/bandage potency.
- **Endurance** (renamed from `Concentration`, see §6.1). Wires the
  long-designed regen multiplier into `src/player/regen.rs`, speeds Exertion
  recovery, and resists hazard/exertion attrition.
- **Survival forage.** A periodic Survival check while in wilderness tiles yields
  food/water, feeding the existing food-buff/regen path.

### 6.1 Exertion — the Medium-sim currency

A per-player **Exertion** meter is the connective tissue between Clusters A/B and
the body loop, and is what gives the game its deliberate, downtime-driven pacing
without survival-chore upkeep.

- **Raised by** exertion-heavy actions: climbing, jumping, pushing/hauling
  (Cluster B), sustained sneaking (Cluster A), and combat.
- **Lowered by** resting (sitting/idle) and food/drink.
- **Governed by attributes** (not a skill, for balance): **Constitution** sets
  the ceiling (`max = 100 + CON_mod·15`, floored), **Willpower** sets the
  recovery rate (`base·(1 + WILL_mod·0.10)`). Presented to the player as a
  depleting **Stamina** bar (`max − current`): full = rested, empty = exhausted.
- **Consequences when high:** physical checks get a fatigue **DC penalty** —
  surfaced via the `Dc` modifier stack so the narration shows it ("vs DC 17
  (climb 15, fatigue +2)") — and HP/mana regen slows. Both push the player
  toward downtime rather than punishing them with death. This is the Medium sim
  depth chosen for the project: *fatigue + rest + optional forage, no
  hunger/thirst/temperature.*

### 6.2 The `Concentration → Endurance` rename

Per `docs/progression.md` §5.3 this is a **pure rename** of the `Skill`
enum variant. The `[u8; 10]` rank-array layout and index are unchanged, so
`GameEvent`s, projection, save data, and the skills UI need only the identifier
rename — no array resize, no migration. The Endurance *skill* governs the
out-of-combat **HP/mana** regen multiplier (`src/player/regen.rs`); the stamina
pool itself is attribute-governed (§6.1), keeping the two levers independent.

---

## 7. Tunable knobs (open numbers)

| Knob | Suggested default | Where |
|---|---|---|
| Sneak movement slow factor | ×1.75 step interval | §3 |
| Sneak Stealth bonus | +5 | §3 |
| Light→detection bonus range | 0 (dark) … +6 (daylight) | §3 |
| Carried-torch detection penalty | strong (treat as bright) | §3 |
| Noise loudness: walk / sneak / door / force / mine / attack | 6 / 0–2 / 5 / 9 / 7 / 10 | §3 |
| Noise field decay | ~1.5 s | §3 |
| NPC base `perception` | 0 (per-NPC authored) | §3 |
| Push Athletics DC per weight | `weight` (kg) as DC | §4 |
| Exertion per heavy action | climb 8 / jump 6 / attack 4 / sneak 2 per s | §6.1 |
| Exertion ceiling / recovery | `100 + CON_mod·15` / `base·(1+WILL_mod·0.1)` | §6.1 |
| Fatigue DC penalty | +1 from 50% spent, +1 per 10%, cap +6 | §6.1 |
| Exertion regen slowdown | ramps to ×0.5 from 75% spent | §6.1 |
| Gather margin → yield/rare | margin/5 extra, rare on margin ≥ 10 | §5 |
| Craft margin → quality tier | +1 per 5 margin, capped | §5 |

---

## 8. Implementation roadmap

The doc is the design; phasing mirrors `docs/progression.md` §9.

### Slice 1 — "Sneak & Seek" (Cluster A) — *first build* — ✅ implemented

Highest impact and the biggest dead connection. Exercises two shared signals
(light, noise) plus a piece of Cluster B (cover). Server-authoritative throughout;
the only client surface is the replicated `Sneaking` flag (HUD) and a one-shot
"spotted" `GameUiEvent`.

1. **Sneaking toggle** — `SetSneaking` command, `Sneaking` marker component
   (transient, not persisted), movement speed penalty, full replication chain
   (`ClientGameState.sneaking`, `GameEvent::PlayerSneakingChanged`, projection
   emit + fold + log), `SneakingLabel` HUD + keybind.
2. **`light_level_at`** — pure helper in `src/world/lighting.rs` (ambient from the
   day/night curve + a nearby-emitter bump), reading authoritative `WorldClock`;
   unit-tested.
3. **Noise bus** — `src/world/noise.rs`: `NoiseEvent`, `PendingNoiseEvents`, a
   decaying `NoiseField`; registered server-only in `GameServerPlugin`; emitted by
   movement/doors/force-lock/mining/combat; NPCs hear it → `AiState::Alert`.
4. **Detection rewrite** — `HostileBehavior.perception`; enrich the detector's
   player slice with stealth/sneaking/identity; opposed Stealth-vs-Perception roll
   in `nearest_visible_player` with light/sneak/cover/noise modifiers; salted
   d20s; a pure `detection_outcome` for tests.
5. **"Spotted" cue** — `GameUiEvent::Spotted` at the fresh aggro transition;
   client toast.

**Deferred (cross-boundary):** Backstab / sneak-attack damage. Combat reads
neither `Class` nor NPC-awareness today; it needs an "is this NPC aware of the
attacker" predicate exposed from npc state into `resolve_battle_turn`. Ship slice
1 first.

### Slice 2 — Exertion + Endurance — ✅ implemented

The Medium-sim currency (§6.1), the Endurance regen wire into `src/player/regen.rs`,
and the `Concentration → Endurance` rename (§6.2).

### Slice 3 — Physical manipulation (Cluster B) — ✅ implemented

Push/pull Athletics gate + Exertion cost, cover contribution into §3, stack-to-climb,
barricade vs NPC pathing, pressure-plate holds. As built:

1. **Push/pull** extends the existing object relocation (`MoveItem` WorldObject →
   WorldTile). A heavy object (`weight > traversal::PUSH_FREE_WEIGHT`) or any
   move beyond an adjacent tile becomes a *push*: `handle_object_push`
   (`src/game/systems.rs`) mirrors `handle_jump_to` — one Athletics roll, a
   line sweep that slides the object to the farthest tile whose distance-scaled
   `traversal::push_dc(weight, tiles)` the roll clears, stopping at the first
   wall/step/collider. Costs `EXERTION_COST_PUSH`, emits `PUSH_NOISE`, settles
   the vacated column. Light objects still drop freely onto an adjacent tile.
2. **Cover, stack-to-climb, barricade** fall out for free — a shoved
   `Collider` already enters the `los_blockers` index (cover, tested in
   `spatial.rs::pushed_collider_provides_cover`), raises `stack_top_z`
   (stack-to-climb, covered by the `resolve_step_with_climb` climb tests), and is
   respected by live A* (barricade, covered by `astar_routes_around_wall`).
3. **Pressure-plate holds** — new `src/world/pressure_plate.rs`
   (`PressurePlate` component + `update_pressure_plates` server system) flips a
   plate between pressed/released on live occupancy that now includes resting
   heavy objects, driving a wired target (door) via `apply_state_transition`.
   Authored via the `pressure_plate:` block (`docs/yaml_formats.md`).

**Deferred (cross-boundary):** Backstab / sneak-attack damage (still per §8's
Slice 1 note).

### Slice 4 — Production & quality (Cluster C)

Gather-margin yield, recipe skill DCs → `ItemModifier` quality tier, Spellcraft
enchant + learn-from-scroll, economy tie-in.

### Slice 5 — Knowledge & body (Cluster D)

Lore / Spellcraft identify, Heal bandage + cure-status, Survival forage.

---

## See also

- `docs/progression.md` — classes, skills list, combat math, death penalty (the *character* side).
- `docs/yaml_formats.md` — object/recipe schema; updated when the `perception` field and recipe `skill` block land.
- `src/player/skills.rs` — `Skill`, `SkillSheet`, `skill_check`.
- `src/world/lighting.rs`, `src/world/hidden.rs`, `src/world/hide_action.rs` — light, hidden-object detection, hiding.
- `src/npc/{components,systems}.rs` — `HostileBehavior`, `AiState`, `nearest_visible_player`, the detection rewrite target.
- `src/magic/effects.rs` — status effects reused by Heal cure-status.
- `src/crafting/` — recipe system extended in Slice 4.
- `src/game/{resources,projection}.rs` — `GameEvent` / `GameUiEvent` / `ClientGameState` replication path.
