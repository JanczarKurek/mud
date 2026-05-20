# Player Progression

Design reference for character growth in Mud 2.0. The system is **D&D 3.5e-flavored** rather than Tibia-style: characters earn XP, level up, gain class features and skill points, and pay a defined cost on death.

This doc is the source of truth for the design. Tuning numbers (HP per level, mana scaling, slot drop chances, XP curve) are explicitly marked as **`[tunable]`** wherever they appear and are gathered in §10. The implementation phasing is in §9 and pointed back at by `PLAN.md` §4.2.

---

## 1. Goals & non-goals

### Goals
- A clear, legible advancement loop: kill / explore / quest → XP → level up → bigger numbers + new options.
- Class identity that actually shapes play (Fighter ≠ Wizard).
- Per-character skill expression beyond raw class.
- A death penalty that hurts but never erases the build.

### Non-goals (deferred)
- PvP-balanced numbers. PvE pacing first; PvP tuning is its own pass.
- Full prestige class roster — §3.5 names the hook only.
- Multi-classing rules.
- Feats. 3.5e's feat tree is a future expansion.
- Equipment-level scaling (magical weapons, +N items).

---

## 2. Attributes

The six attributes already exist (`src/player/components.rs:234-282` — `AttributeSet`). They map onto D&D's classic six:

| Mud 2.0 | D&D 3.5e equivalent | Primary effects |
|---|---|---|
| Strength (STR) | STR | Melee to-hit, melee damage, Athletics, carry capacity |
| Agility (AGI) | DEX | Ranged to-hit, AC (dex-mod), Reflex saves, Stealth, Thievery |
| Constitution (CON) | CON | Max HP, Fortitude saves, Endurance |
| Willpower (WIL) | WIS | Max mana (caster classes), Will saves, Perception, Heal, Survival |
| Charisma (CHA) | CHA | Persuasion, NPC reactions, divine spell DC (placeholder) |
| Focus (FOC) | INT | Skill points per level, Lore, Spellcraft, arcane spell DC (placeholder) |

Attribute scores are **integers ≥ 1**, default 10 at character creation, modifier formula:

```
modifier = (score - 10) / 2     (integer division, rounded toward -∞)
```

Examples: 10 → +0, 12 → +1, 14 → +2, 8 → -1, 6 → -2.

The current formulas in `DerivedStats::from_base` (`src/player/components.rs:349-365`) compute starting HP/mana from attributes. They stay as the **level-0 base** — leveling adds on top, never replaces.

---

## 3. Classes

Four base classes ship at v1: **Fighter**, **Wizard**, **Cleric**, **Vagabond**. A character picks one at creation and stays single-class for now.

### 3.1 Fighter

Front-line martial. Soaks hits, hits hard, doesn't cast.

| | |
|---|---|
| Hit Die | **d10** `[tunable]` |
| BAB progression | Full (`+1 / level`) |
| Saves | **Fort** good, Ref poor, Will poor |
| Skill points / level | 2 + FOC mod (min 1) |
| Class skills | Athletics, Endurance, Perception, Survival |
| Casting | None (mana stays at level-0 base) |
| Starting feature | **Weapon Focus**: +1 to melee to-hit at level 1; +1 again at level 5 and every 5 thereafter `[tunable]` |

### 3.2 Wizard

Arcane caster. Fragile, mana-rich, scales hard.

| | |
|---|---|
| Hit Die | **d4** `[tunable]` |
| BAB progression | Half (`+1 / 2 levels`) |
| Saves | Fort poor, Ref poor, **Will** good |
| Skill points / level | 2 + FOC mod (min 1) |
| Class skills | Spellcraft, Lore, Stealth, Endurance |
| Casting | Arcane (FOC-keyed); see §6 |
| Starting feature | **Spellbook**: starts knowing 2 cantrips + 1 first-level arcane spell; learns one new spell per level on level-up `[tunable]` |

### 3.3 Cleric

Divine caster. Mid martial, full healer/support.

| | |
|---|---|
| Hit Die | **d8** `[tunable]` |
| BAB progression | Three-quarter (`+3 / 4 levels`, see §7.4) |
| Saves | **Fort** good, Ref poor, **Will** good |
| Skill points / level | 2 + FOC mod (min 1) |
| Class skills | Heal, Lore, Persuasion, Spellcraft, Perception, Survival |
| Casting | Divine (WIL-keyed); see §6 |
| Starting feature | **Domain**: pick one (e.g. War / Healing / Trickery — actual roster TBD). Grants a thematic class spell per level `[tunable]` |

### 3.4 Vagabond

Skill specialist, opportunistic damage. Mud 2.0's flavor for the 3.5e Rogue.

| | |
|---|---|
| Hit Die | **d6** `[tunable]` |
| BAB progression | Three-quarter |
| Saves | Fort poor, **Ref** good, Will poor |
| Skill points / level | **8 + FOC mod** (min 1) |
| Class skills | Stealth, Thievery, Perception, Persuasion, Athletics, Survival, Lore |
| Casting | None at base (advanced classes may unlock) |
| Starting feature | **Backstab**: +1d6 damage on attacks where target hasn't yet acted in combat or is unaware. Scales +1d6 every 4 levels `[tunable]` |

### 3.5 Advanced classes (deferred)

Hook only. The intent is 3.5e prestige-class shape: a character meets a set of prereqs (level threshold + skill ranks + class feature) and unlocks an advanced class for further levels. **No advanced classes are designed yet.** When they land, this section gets a per-class entry with prereqs, HD/BAB/save shifts, and unique features.

---

## 4. Leveling

### 4.1 XP curve

Cumulative XP needed to **be** level N (3.5e standard):

```
xp_for_level(N) = 1000 × N × (N - 1) / 2     [tunable: 1000 coefficient]
```

| Level | Cumulative XP |
|---:|---:|
| 1 | 0 |
| 2 | 1,000 |
| 3 | 3,000 |
| 4 | 6,000 |
| 5 | 10,000 |
| 10 | 45,000 |
| 20 | 190,000 |

**Initial level cap: 20** `[tunable]`.

### 4.2 XP awarded on kill

Placeholder formula (gross simplification of 3.5e's CR system):

```
xp_grant = victim_level² × 50     [tunable]
```

Awarded to the entity holding the killing blow's `attacker` slot in `resolve_battle_turn` (`src/combat/systems.rs:72`). If the killer is an NPC, no grant. The XP grant emits an `ExperienceGained { amount }` GameEvent (see §9).

### 4.3 What a level-up gives

When `current_xp ≥ xp_for_level(level + 1)`, the character levels:

1. **HP**: gain `roll(HitDie) + CON_mod`, minimum 1. Average is fine for v1 (`floor(HD/2) + 1 + CON_mod`); explicit roll is a future variant. `[tunable]`
2. **Mana** (caster classes): gain `class_mana_per_level + casting_mod`, minimum 0. `[tunable]`
   - Wizard: 10/level, casting_mod = FOC_mod
   - Cleric: 8/level, casting_mod = WIL_mod
   - Vagabond: 0/level (until advanced class)
   - Fighter: 0/level
3. **Skill points**: as listed per class. Spent immediately or banked.
4. **BAB / save bonuses**: recomputed from class progression (see §7).
5. **Ability score bump**: at levels 4, 8, 12, 16, 20 — player picks one attribute and adds +1.
6. **Class feature thresholds**: per-class effects fire at specific levels (Fighter weapon focus at 5/10/15/20, Vagabond backstab dice at 4/8/12/16/20, Wizard new spells per level, Cleric domain spells, etc.).
7. Emit `LevelUp { new_level }` GameEvent and a `GameUiEvent::LevelUpToast` for the HUD.

A level-up is **never declined or deferred** — it applies immediately when the threshold trips. Spending skill points and choosing the ability bump are async — they sit in a "pending choice" state until the player commits via the UI.

---

## 5. Skills

Ten skills total. Combat power is class/BAB-driven (§3, §7), so **skills are purely
utility** — each skill exists to make a non-combat decision matter and has exactly one
concrete, server-hookable mechanic. Each is keyed to one attribute:

| Skill | Attr | What it does in mud2.0 |
|---|---|---|
| Athletics | STR | Climb/jump/swim tiles flagged for it; force locks/doors (`force_dc`); escape immobilizing effects (paralyze/snare) faster; reposition/flee check to break or close distance. |
| Endurance | CON | *(renamed from Concentration)* Faster out-of-combat HP/mana regen (multiplier on `src/player/regen.rs`); shorter rest downtime; resists hazard-tile and hunger/thirst attrition. The front-line martial's payoff: less downtime between fights. |
| Perception | WIL | Detect hidden objects/NPCs and traps before they trigger; ambush warning; reveal hidden tile contents. Opposed by Stealth. |
| Stealth | AGI | Reduces NPC `HostileBehavior` detection radius for the local player; enables sneaking past and setting up the Vagabond **Backstab** class feature (Stealth enables the opening; the bonus damage stays class-driven). Opposed by Perception. |
| Thievery | AGI | Pick locks (`pick_dc`); disarm traps; pickpocket / sleight-of-hand against NPC inventories; hide objects. |
| Survival | WIL | Forage food/water from terrain; track NPC trails across tiles; safe passage through wilderness hazards. (Field/exploration — distinct from Endurance's body recovery.) |
| Spellcraft | FOC | Magic *utility*, **not** spell damage: identify and learn spells from scrolls (feeds the Wizard Spellbook feature), identify magical auras/effects, scroll/enchant crafting. |
| Heal | WIL | First-aid **bandage** action restores HP out of combat; **cure status** (poison/disease) skill check; multiplies potion/bandage potency. |
| Lore | FOC | Identify unknown items (reveal stat/value panel); recognize NPCs/monsters (reveal their panel + weaknesses); appraisal sets the best base price (distinct from Persuasion's live haggle); lore dialog gates. |
| Persuasion | CHA | Diplomacy / Bluff / Intimidate consolidated. Affects merchant prices (`src/game/trade.rs`), Yarn dialog branches, talking hostile NPCs down, and intimidating weaker NPCs to flee. |

### 5.1 Ranks, points, and caps

- Each rank in a skill costs **1 skill point** if it's a class skill, **2 skill points** if cross-class.
- **Max ranks** = `level + 3` for class skills, `floor((level + 3) / 2)` for cross-class.
- A skill check rolls:

```
skill_check_total = d20 + ranks + ability_mod + situational
```

vs a target DC. Common DCs `[tunable]`:

| DC | Difficulty |
|---:|---|
| 5 | Trivial |
| 10 | Easy |
| 15 | Moderate |
| 20 | Hard |
| 25 | Very hard |
| 30+ | Heroic |

### 5.2 Class skill recap

| Class | Class skills |
|---|---|
| Fighter | Athletics, Endurance, Perception, Survival |
| Wizard | Spellcraft, Lore, Stealth, Endurance |
| Cleric | Heal, Lore, Persuasion, Spellcraft, Perception, Survival |
| Vagabond | Stealth, Thievery, Perception, Persuasion, Athletics, Survival, Lore |

### 5.3 Implementation deltas (doc leads the code)

This redesign is design-only; the code has not been changed yet. For the future
implementation effort:

- **Rename** the `Skill::Concentration` enum variant to `Skill::Endurance`. The
  `[u8; 10]` skill-rank layout and index are **unchanged** (pure rename), so
  `GameEvent`s, projection, save data, and the skills UI need only the identifier
  rename — no array resize, no migration.
- Six skills currently have **no mechanic in code** (Endurance, Perception beyond
  hidden-object spotting, Stealth, Survival, Spellcraft, Heal, Lore). §5 now pins
  exactly one server-hookable mechanic per skill — implement them in an impact-ordered
  phased pass (suggested first: Endurance regen multiplier in `src/player/regen.rs`
  and Heal's first-aid action, since they benefit the most under-served classes).
- `PLAN.md` Phase 6 §C marks "Skills shipped"; it now needs a one-line follow-up that
  the *mechanics* are pending per the redesigned §5 (keeps `PLAN.md` ↔
  `docs/progression.md` consistent, per CLAUDE.md).

---

## 6. Magic & casting

Mana stays as the casting resource (`VitalStats.max_mana` at `src/player/components.rs:284-301`). The current YAML schema in `src/magic/resources.rs:8-19` (`SpellDefinition`) gains two fields:

```yaml
# assets/spells/spark_bolt.yaml
name: Spark Bolt
incantation: Exori Vis
mana_cost: 12.0
targeting: targeted
range_tiles: 5
class_access: [Wizard]        # NEW — list of classes that can cast
min_caster_level: 1           # NEW — required class level
effects:
  damage: 18.0
```

`docs/yaml_formats.md` is updated alongside Phase E.

### 6.1 Mana scaling

Replace the static formula with a level-scaled one for casters:

```
max_mana = base_mana + level × (class_mana_per_level + casting_mod)     [tunable]
```

Where `base_mana` is the existing `DerivedStats::from_base` mana value. Class factors are in §4.3 step 2.

### 6.2 Cleric vs Wizard split

Spell access is per-class. A spell's `class_access:` lists every class that can cast it. Wizard spell list and Cleric spell list overlap intentionally (e.g. "Light" is on both). Damage-dealing arcane spells default to Wizard-only; healing/buff/divine-flavored spells default to Cleric-only.

### 6.3 Casting under pressure (deferred)

There is **no Concentration skill** (the skills redesign removed it — see §5). Resisting
cast interruption is combat math, and combat stays class/BAB-driven, so if cast times
ever land the interrupt check is an attribute/class-feature roll (`d20 + CON mod` vs
DC `10 + damage_taken`, failure consumes mana for no effect), **not** a skill check and
not modified by Spellcraft. For now all casts are instant, so this is fully deferred.

---

## 7. Combat math

Replaces the current `resolve_battle_turn` formula at `src/combat/systems.rs:72` (today: `d6 + str/5` flat).

### 7.1 To-hit

```
attack_roll  = d20 + BAB + ability_mod + situational
hit if attack_roll ≥ target_AC
natural 20 = always hit (and threatens crit), natural 1 = always miss
```

- `ability_mod` = STR_mod for melee, AGI_mod for ranged.
- `BAB` is per-class and per-level — see §7.4.
- `situational` rolls in flanking, height advantage, etc. — placeholders for now.

### 7.2 AC

```
AC = 10 + AGI_mod + armor + shield + dodge
```

- `armor` = flat reduction from equipped torso/legs/head — values `[tunable]`, will be defined when armor stats land in the combat-depth batch.
- `shield` = flat from equipped shield slot.
- `dodge` = situational; default 0.

**Refinement (implemented):** The combat-depth batch split these channels — armor and shield no longer contribute to the to-hit DC. Instead the dodge DC is `10 + AGI_mod + sum(item.dodge_bonus)`, and `armor` mitigates damage *post-hit* (additive subtract, same as today's flow), while `shield`'s mitigation is *chance-gated* by a per-shield `block_chance` roll (default `block_chance + AGI_mod * 2`, clamped to `[0, 95]`). See `src/combat/systems.rs::resolve_battle_turn`.

### 7.3 Damage

```
weapon_damage = roll(weapon_damage_expr) + STR_mod
two-handed: STR_mod × 1.5 (rounded down)
```

`weapon_damage_expr` lives on the equipped weapon definition (already wired via `WeaponDamage` component referenced at `src/combat/systems.rs:85`).

### 7.4 BAB and save tables

| Level | Fighter BAB | 3/4 BAB (Cleric / Vagabond) | Half BAB (Wizard) |
|---:|---:|---:|---:|
| 1 | +1 | +0 | +0 |
| 2 | +2 | +1 | +1 |
| 3 | +3 | +2 | +1 |
| 4 | +4 | +3 | +2 |
| 5 | +5 | +3 | +2 |
| 10 | +10 | +7 | +5 |
| 20 | +20 | +15 | +10 |

Saves use the standard 3.5e Good/Poor progression:
- **Good** save at level N: `2 + N/2`
- **Poor** save at level N: `N/3`

(Both rounded down. Add the relevant ability mod when rolling.)

---

## 8. Death & penalty

Trigger: existing `PendingPlayerDeaths` queue at `src/player/lifecycle.rs:26`, drained by `handle_player_deaths` at `src/player/lifecycle.rs:62`.

### Current behavior (today)

`drain_inventory` at `src/player/lifecycle.rs:158` empties **everything** (backpack + all equipment) onto a corpse spawned by `spawn_corpse_for_player`, then teleports the player home with full HP/MP. There is no XP loss — there's no XP yet.

### New behavior (this design)

Three rules apply on death, in order:

**Rule 1 — XP zeroing (no de-leveling).**
```
new_xp = xp_for_level(current_level)
```
Effectively: all progress *into* the current level is lost, but level number never decreases. A character who just hit level 7 (35,000 XP) and dies stays at level 7 with 21,000 XP — all 14,000 of progress toward level 8 is gone, but they don't drop to level 6.

**Rule 2 — Backpack always drops.**
Iterate `Inventory.backpack_slots`. Every stack moves to the corpse. (Same as `drain_inventory` does today for the backpack portion.)

**Rule 3 — Equipped slots roll independently.**
For each slot in `Inventory.equipment_slots`:
```
if rng.gen_range(1..=100) ≤ slot_drop_chance_percent:
    move slot to corpse
```
Default per-slot chance: **10%** `[tunable]`. AOL-style protection (an equipped item that grants immunity to drop) is a future hook — note it on the slot's item definition and short-circuit the roll.

Other death effects (full-vitals respawn, teleport home, clear regen tickers, drop combat target) stay exactly as they are today.

### 8.1 Where this hooks in the code

- The XP-zero rule: in `handle_player_deaths` at `src/player/lifecycle.rs:62`, after the player entity is fetched, before `spawn_corpse_for_player` is called.
- The backpack-always / equipment-roll split: rewrite `drain_inventory` at `src/player/lifecycle.rs:158` to take a slot-drop-chance parameter and roll per equipment slot.
- An `ExperienceLost { amount }` GameEvent fires for the HUD; a `GameUiEvent::DeathSummary { items_dropped, xp_lost }` fires the death recap dialog.

---

## 9. Implementation roadmap

The doc is the design; the phasing is the execution plan. `PLAN.md` §4.2 (Phase 6) points here.

### Phase A — XP + Level ✅ *shipped*

Implemented in `src/player/progression.rs` (`Experience`, `xp_for_level`,
level-up system); XP grant on kill in `src/combat/systems.rs`; bar/toast in
`src/ui/`. Remainder of this subsection kept for context.

Smallest standalone increment that produces a visible loop.

- New `Experience { current_xp: u64, level: u32 }` component on `Player` entities — file `src/player/progression.rs` (new).
- `xp_for_level(N)` helper + level-up detection system that runs after combat.
- New GameEvent variants in `src/game/resources.rs:151`: `ExperienceGained { amount }`, `LevelUp { new_level }`, `ExperienceLost { amount }`.
- XP grant: in `resolve_battle_turn` at `src/combat/systems.rs:72`, when an attack reduces target HP ≤ 0 and the attacker is a player, push an XP grant.
- Persistence: extend `PlayerStateDump` at `src/persistence/mod.rs:155-186` with `experience: Experience` field (`#[serde(default)]` for back-compat with saves written before this lands). The accounts-DB schema at `src/accounts/db.rs:78-93` doesn't change — XP rides inside `state_json`.
- HUD: an XP bar component reading `ClientGameState`. New `ClientGameState.experience` field.
- Toast: `GameUiEvent::LevelUpToast { new_level }` for the level-up notification.

### Phase B — Classes ✅ *shipped*

Implemented in `src/player/classes.rs` (`Class` enum + per-class data tables),
`ChooseClass` command (`src/game/commands.rs`, handled in
`src/game/systems.rs`), class-picker buttons in `src/ui/systems.rs`. Remainder
of this subsection kept for context.

- `Class` enum (Fighter / Wizard / Cleric / Vagabond) — `src/player/classes.rs` (new).
- Per-class data tables (HD, BAB progression, save profile, skill points/level, class skill list, mana growth) live in `classes.rs` as const lookup arrays.
- `ClassChoice` component on `Player` (single field for now; multi-class is later).
- `GameCommand::ChooseClass { class }` in `src/game/commands.rs:63` — used at character creation.
- `DerivedStats::from_base` updated to accept class + level and apply the per-level bumps (HP, mana, BAB, saves). Renames may follow.
- Character creation UI: a class-select panel before the player enters the world.

### Phase C — Skills

- `SkillSheet` component (`HashMap<SkillId, u8>` or fixed-size array) — `src/player/skills.rs` (new).
- `SkillId` enum + per-skill metadata (associated attribute, default DCs).
- `GameCommand::AssignSkillPoint { skill_id }` in `src/game/commands.rs:63`.
- Bank of unspent skill points lives on the same component.
- `skill_check(player, skill_id, dc) -> SkillCheckResult` helper used by future systems (Stealth detection in NPC AI, Perception in object hover, etc.).
- New GameEvent: `SkillRanksChanged { skill_id, new_ranks }`.
- UI: skills tab in the character sheet docked panel.

### Phase D — Death penalty ✅ *shipped*

Implemented in `src/player/lifecycle.rs` (`drain_inventory_with_drop_chance`,
`SLOT_DROP_CHANCE_PERCENT`, XP-zero rule), `DeathSummary` GameUiEvent +
overlay in `src/ui/systems.rs`. Remainder of this subsection kept for context.

- Rewrite `drain_inventory` at `src/player/lifecycle.rs:158` to split backpack-always vs equipment-roll.
- XP-zero rule applied in `handle_player_deaths` at `src/player/lifecycle.rs:62`, before corpse spawn.
- `GameUiEvent::DeathSummary { items_dropped: Vec<...>, xp_lost: u64 }` for the recap dialog.

### Phase E — Magic gating

- Extend `SpellDefinition` at `src/magic/resources.rs:8` with `class_access: Vec<Class>` and `min_caster_level: u32` fields (`#[serde(default)]`).
- Update each YAML in `assets/spells/` with the new fields.
- `cast_spell` (the spell handler in `combat/systems.rs` or magic systems) checks class + caster_level before spending mana.
- Mana scaling formula (§6.1) wired into per-level recompute on level-up.
- Update `docs/yaml_formats.md` spell schema section.

### Phase F (deferred)

- Advanced/prestige class roster + per-class detail.
- Multi-classing rules (level allocation across classes; BAB/save aggregation).
- Feats.
- Equipment-level scaling (magical weapons).

---

## 10. Tunable knobs (open numbers)

Single-source list of every `[tunable]` referenced above. When a number lives here, future tuning passes can grep for it.

| Knob | Default | Where it's used |
|---|---|---|
| Hit Die per class | d10 / d4 / d8 / d6 (F/W/C/V) | §3, §4.3 |
| Skill points / level | 2 / 2 / 2 / 8 | §3 |
| XP curve coefficient | 1000 | §4.1 |
| Level cap | 20 | §4.1 |
| XP awarded per kill | `victim_level² × 50` | §4.2 |
| Mana per level (caster classes) | Wizard 10, Cleric 8 | §4.3, §6.1 |
| Ability bump cadence | every 4 levels | §4.3 |
| Skill DC anchors | 5 / 10 / 15 / 20 / 25 / 30 | §5.1 |
| Slot drop chance on death | 10% per equipment slot | §8 rule 3 |
| Fighter Weapon Focus bump | +1 at L1, L5, L10, L15, L20 | §3.1 |
| Vagabond Backstab dice | +1d6 / 4 levels | §3.4 |
| Cast-interrupt DC (deferred) | `10 + damage_taken`, CON-keyed | §6.3 |

---

## See also

- `PLAN.md` §4.2 — Phase 6 of the project roadmap; points back here.
- `FEATURE_BACKLOG.md` §1 "Progression" — collapsed to a pointer to this doc.
- `src/player/components.rs:234-365` — existing stat layer this design extends.
- `src/player/lifecycle.rs:26-185` — death queue and current `drain_inventory` (the Phase D rewrite target).
- `src/combat/systems.rs:72` — `resolve_battle_turn`, the XP-grant injection point and the to-hit/damage rewrite target.
- `src/game/resources.rs:151-232` — `GameEvent` enum (Phase A/B/C add variants here).
- `src/game/commands.rs:63-200` — `GameCommand` enum (Phase B/C add variants here).
- `src/persistence/mod.rs:155-186` — `PlayerStateDump`, extended in Phase A.
- `src/magic/resources.rs:8-19` — `SpellDefinition`, extended in Phase E.
- `docs/yaml_formats.md` — kept in sync with the spell schema additions in Phase E.
