# Combat & Stats Balance Report

_Generated from a faithful Python re-implementation of the live combat math
(`tools/balance/`). Every number below traces to a quoted formula or a cell in
`adventurers.csv` / `creatures.csv` / `matchups.csv` / `generated_tables.md`._

> **Scope.** This is an **analysis**, not a code change. Nothing in the game was
> modified. The models reproduce the *implemented* rules (verified against the
> Rust source and its unit tests), which in several places differ from
> `docs/progression.md`. Those gaps are findings, not bugs in this report.

---

## TL;DR

The numbers are roughly tuned for **level 1**, and drift out of balance fast:

1. **A player's offence never grows.** To-hit = `ability_mod(STR/AGI)` (capped at
   **+4**) and weapon damage come *only* from attributes + gear. Players get **no
   BAB**, and **attributes never rise on level-up** — only HP and mana scale. So a
   level-20 character swings exactly as hard and as accurately as at level 1.
2. **Every creature's offence grows +1 to-hit per level.** By ~level 8 a creature
   hits the player on essentially every swing (the real **Cyclops, L8, hits 100%**),
   no matter how much AGI the player buys. Creature HP and damage scale too.
3. **The result:** fights are winnable at L1, already coin-flippy at L5 for
   un-optimised builds, and lost against level-appropriate enemies by L10+. Against
   the shipping **Cyclops (L8)**, a sword-and-board STR Fighter *loses*; only
   full-strength-scaling weapons win.
4. **The starter melee weapon is the worst weapon in the game.** The Bronze Sword
   uses the default `1d6 + strength/5`; the **Pickaxe** (a mining tool) uses
   `1d4 + strength`. At STR 18 that's **6.5 vs 20.5** average damage. Same fighter
   vs the same Cyclops: **5.5 DPS → loss** with the sword, **18.5 DPS → win** with
   the pickaxe.
5. **Armor is swingy and block is cosmetic.** Armor subtracts a *random* `0..N`
   (so a roll of 0 lets a full hit through); full leather (armor 7) halves a small
   hit but barely dents a big one. A wooden shield removes **< 0.5 damage per hit**.
6. **Magic is burst-only.** Spell damage is flat (no FOC/level scaling); mana regen
   is ~0.09/s, so **sustained spell DPS is ~0.1**. A caster empties its bar for a
   big burst, then is a bad meleer for minutes.

The single highest-leverage fix already exists in code but is unused: `bab_at()`
(`src/player/classes.rs:148`) computes a per-class, per-level attack bonus —
**wiring it into player to-hit, and capping the NPC `+level` term to the same
track, closes most of the asymmetry.** Details in §5.

---

## 1. How combat actually works (verified)

One attack is resolved per **battle turn**, and the battle-turn timer fires once
per second (`src/combat/resources.rs`), so **1 turn = 1 second** of real combat.
Resolution order (`src/combat/systems.rs:457-504`):

| Stage | Formula | Source |
|---|---|---|
| **To-hit** | `d20 + atk_bonus ≥ 10 + AGI_mod(def) + dodge_items` | formulas.rs:9, :24 |
| `atk_bonus` | `ability_mod(STR melee / AGI ranged) + (level if NPC else 0)` | formulas.rs:24 |
| **Damage** | `max(1, weapon_expr.roll())` | systems.rs:474 |
| **Block** (if shield) | chance `clamp(block_chance + AGI_mod·2, 0, 95)%`; then `dmg -= randint(0, block)` | formulas.rs:16, systems.rs:488 |
| **Armor** | `dmg -= randint(0, armor)`, floored at 1 | systems.rs:503 |

Key facts the model encodes:

- **`ability_mod(score) = (score-10)//2`** (toward −∞); attributes cap at 18 at
  creation → **+4 max** (classes.rs:168).
- **No natural-20 / natural-1 rule.** A straight d20 vs DC (so to-hit can be a
  guaranteed hit or guaranteed miss purely from the modifiers).
- **Players get no BAB.** The `+level` term applies to **NPCs only**
  (formulas.rs:24, comment included).
- **Attributes are static.** Level-up grants HP, (caster) mana, and skill points
  only — there is **no automatic ability-score increase** (the L4/8/12/16/20 bumps
  in `progression.md` §4.3 are **not implemented**; verified in
  `player/progression.rs` and `player/admin_progression.rs`).
- **Armor / block are random** uniform draws, mean `N/2`, applied only in the
  weapon path. **Spell and DoT damage bypass armor and block entirely** — they
  push a `DamageEvent` with flat damage (`magic/effects.rs`, `combat/damage.rs:146`).
- **Damage types are cosmetic** — no resistance/weakness affects mitigation
  (`combat/damage_type.rs`).
- **No two-handed 1.5× STR**, and weapons with no `damage:` field fall back to
  `1d6 + strength/5` (`combat/damage_expr.rs:63`).

### Where the code differs from `docs/progression.md`

| progression.md says | Code actually does |
|---|---|
| To-hit includes BAB (§7.1, §7.4) | Players get **no** BAB; only NPCs add `+level` |
| Ability bump every 4 levels (§4.3) | **Not implemented** — attributes never change after creation |
| Two-handed `STR_mod × 1.5` (§7.3) | Not implemented |
| Damage types matter | Cosmetic only |
| HP default ≈ "from_base" | All-10 Fighter L1 = **115 HP** (`35 + CON·6 + STR·2`), **100 mana** |

---

## 2. The numbers today

Full tables in [`generated_tables.md`](generated_tables.md); the load-bearing ones:

### 2.1 Accuracy is flat and capped (§1 of generated tables)

A player's whole to-hit bonus is `ability_mod` — at best **+4**, and it never
grows. Against a defender it's a flat slice of the d20:

| atk bonus → | vs AGI 10 (DC10) | vs AGI 14 (DC12) | vs AGI 18 (DC14) |
|---|---|---|---|
| +0 (STR 10) | 55% | 45% | 35% |
| +4 (STR 18) | 75% | 65% | 55% |

### 2.2 …but creature accuracy scales without bound (§2)

NPC to-hit = `ability_mod + level`. Player dodge tops out around DC 14–15, so:

| NPC level | hits a DC-10 player | hits a DC-15 player (AGI 18 + boots) |
|---|---|---|
| 3 | 80% | 55% |
| 8 | **100%** | 80% |
| 14 | 100% | **100%** |

This is **confirmed by shipping content**: the Cyclops (real, L8) hits **every**
adventurer build at 100% in `matchups.csv`.

### 2.3 Armor is swingy, block is cosmetic (§3, §4)

Armor subtracts a random `0..N` (mean `N/2`), floored at 1 damage:

| armor | vs 6 dmg | vs 25 dmg |
|---|---|---|
| 3 (leather body) | 4.5 (−25%) | 23.5 (−6%) |
| 7 (full leather) | 2.5 (−58%) | 21.5 (−14%) |

A Wooden Shield (block 3, 25% chance) removes **0.35–0.49 damage per hit** even at
high AGI — effectively decorative.

### 2.4 The weapon table is inverted (§5)

Mean damage by attribute — note the *tools* beat the *sword*:

| weapon (expr) | STR/AGI 10 | STR/AGI 18 |
|---|---|---|
| **Bronze Sword** `1d6+STR/5` (starter) | 5.5 | 6.5 |
| Pickaxe `1d4+STR` (tool) | 12.5 | **20.5** |
| Shortbow `1d6+STR` (to-hit uses AGI!) | 13.5 | 21.5 |
| Crossbow `2d4+AGI` | 15.0 | **23.0** |

The Bronze Sword's `strength/5` term means STR barely helps it; the bow scales
**damage off STR but to-hit off AGI**, so an AGI archer hits often but hits soft.

### 2.5 Magic is pure burst (§6)

Spell damage is flat (no FOC/level scaling); only the mana pool grows. With WIL-16
regen (~0.087 mana/s):

| spell | mana | dmg | dmg/mana | sustained DPS |
|---|---|---|---|---|
| Spark Bolt | 12 | 18 | **1.50** | 0.13 |
| Magic Dart | 4 | 5 | 1.25 | 0.11 |
| Fireball | 22 | 14 | 0.64 | 0.06 |

A L20 glass wizard's ~426-mana pool is a one-shot **burst** of ~630 damage (35
Spark Bolts), then ~0.1 DPS until it regenerates over *minutes*. Spark Bolt is also
a clear efficiency **outlier** (18 flat damage for 12 mana at min level 1).

### 2.6 Only HP/mana move with level (§7), but XP comes faster (§8)

| class | HP L1 → L20 | offence L1 → L20 |
|---|---|---|
| Fighter | 151 → 303 | **flat** |
| Wizard | 103 → 141 | **flat** |
| Cleric | 143 → 276 | **flat** |

Leveling actually *accelerates* (kills-to-next-level: 20 at L1 → 7 at L3 → 2 at
L10), because `xp_grant = level²·50` outpaces the `1000·n(n−1)/2` curve. So players
out-level the content's *XP gates* while falling behind its *combat math*.

### 2.7 Headline duels (Monte-Carlo, 2500 trials each)

Each build vs a level-appropriate creature (win = player alive, creature dead).
Real shipping creatures grounded separately below.

| build | L1 (vs Rat) | L5 (vs Orc Brute*) | L10 (vs Stone Golem*) |
|---|---|---|---|
| Generalist (sword) | 100% | 12% | 0% |
| STR Fighter (sword+shield) | 100% | 93% | 0% |
| **Brute (pickaxe)** | 100% | 100% | **23%** |
| **Archer (crossbow)** | 100% | 100% | 0% |
| Glass Wizard (melee fallback) | 100% | 0% | 0% |
| Balanced Cleric (sword) | 100% | 13% | 0% |

`*` = designed filler creature (see §6 caveats). Grounded in **shipping** creatures:

| build, level | vs Fire Elemental (real L6) | vs Cyclops (real L8) |
|---|---|---|
| STR Fighter L5 (sword) | win (slow: 30s) | **LOSS** (5.5 DPS, dies in 16s) |
| Brute L5 (pickaxe) | win (8s) | **WIN** (18.5 DPS, kills in 11s) |
| Archer L5 (crossbow) | win (7s) | **WIN** (20.7 DPS) |
| Glass Wizard L5 (melee) | loss | loss |

> The Cyclops result is the whole report in one line: the **only difference**
> between the STR Fighter (loss) and the Brute (win) is the **weapon** — sword vs
> pickaxe. Combat is decided by gear scaling the player can't see in the UI.

---

## 3. Diagnosed problems (ranked by impact)

1. **Player offence is frozen across 20 levels.** No BAB, no attribute growth →
   to-hit and damage are identical at L1 and L20. This is the root cause of the
   "numbers feel bad" drift.
2. **NPC to-hit scales `+1/level` with no cap**, so mid/high-level creatures
   auto-hit. The defence side (dodge DC) has no level term to answer it.
3. **The default melee weapon (`1d6+strength/5`) is mis-scaled** and is beaten by
   mining tools. Weapon identity is incoherent (sword < pickaxe; bow damage keys
   off the wrong attribute).
4. **Armor is random and unscaled.** The `0..N` roll makes low-damage hits swingy,
   and no armor above ~7 total exists, so it's irrelevant to high-level damage.
5. **Block is statistically negligible** (<0.5 dmg/hit) — a whole defensive system
   that does nothing.
6. **Magic has no damage scaling and starves on mana**, collapsing to burst-only
   with an efficiency outlier (Spark Bolt). Caster level adds casts, not power.
7. **New-player trap builds.** A legal "bump everything" generalist (all-12) or any
   sword user is already losing by L5; the game never signals that weapon/attribute
   choice is what's killing them.

---

## 4. What good numbers look like (recommended ranges)

Framed as recommendations; **no changes applied**. Pick the cheap wins first.

### 4.1 Restore player offence scaling (highest leverage)

- **Wire `bab_at(track, level)` into player to-hit.** It already exists
  (`classes.rs:148`) and is unused. Add it in
  `formulas::attack_to_hit_bonus` for players. Result: Fighter +1/level, Cleric/
  Vagabond +¾/level, Wizard +½/level — the missing L1→L20 accuracy curve, for free.
- **Make the NPC term symmetric.** Replace the raw `+level` with
  `bab_at(npc_track, level)` (most creatures ¾ or ½; brutes full). This alone stops
  the auto-hit problem while keeping fighters scary.
- If you want damage to scale too (recommended for casters/martials alike),
  implement the **ability bump every 4 levels** that `progression.md` §4.3 already
  specifies but the code skips.

### 4.2 Re-tier weapons

Give every weapon an explicit `damage:` and a coherent scaling slope. Suggested
martial template (1-handed), damage roughly `dice + STR/2`:

| tier | example | suggested expr | mean @STR14 |
|---|---|---|---|
| starter | Bronze Sword | `1d8+strength/2` | ~11 |
| iron | (new) | `1d10+strength/2` | ~12.5 |
| steel | (new) | `2d6+strength/2` | ~14 |

- **Bows should key damage off AGI** (or accept STR bows but say so) so the
  to-hit and damage attributes match. Crossbow (`2d4+agility`) is already coherent —
  use it as the model.
- **Tools** (`pickaxe`, `herb_knife`) should *not* out-damage weapons — drop them to
  `1d4+strength/3` or similar so they stay useful utility, not best-in-slot.

### 4.3 Make defence meaningful and scalable

- **De-swing armor:** use flat `N/2` (deterministic) or a tighter band, so a 0-roll
  doesn't negate the armor. Keep the min-1 floor.
- **Ship armor that scales:** total worn armor should reach ~`level` for a dedicated
  tank (e.g. plate sets of 6–10 per body piece) so it stays relevant against
  double-digit hits.
- **Fix block:** make a successful block a **percentage** cut (e.g. 30–50%) or a
  flat reduction equal to the full `block` value (not `0..block`), and let shields
  contribute meaningful `block_chance` (40–60% for a tower shield).

### 4.4 Decide magic's identity

- If **burst** is intended: keep flat damage but **document it**, re-tier so
  dmg/mana is monotonic (Spark Bolt 18/12 is out of line vs Magic Dart 5/4), and
  speed up mana regen or add wands for between-fight damage.
- If casters should **scale**: add a `+FOC_mod` (or `+caster_level/2`) term to
  spell `damage`, mirroring weapon STR scaling, so a L20 wizard hits harder per
  cast, not just more often.

### 4.5 Creature budget template

So creatures stop being hand-tuned, anchor them to level. These slopes match the
shipping curve (Rat→Cyclops) and the §6 fillers:

| field | suggested budget |
|---|---|
| HP | `~25 + level·18` (tanky `×1.4`, fragile `×0.7`) |
| damage mean/hit | `~3 + level·1.5` |
| to-hit | `bab_at(track, level) + ability_mod` (not raw `+level`) |
| armor | `0` early; up to `~level/3` for armored types |
| XP | leave `level²·50` — pacing is fine |

### 4.6 Suggested creation spreads (so new players don't trap themselves)

All legal 12-point buys; offence-relevant attribute first:

| class | STR | AGI | CON | WIL | CHA | FOC |
|---|---|---|---|---|---|---|
| Fighter | 16 | 12 | 14 | 10 | 10 | 10 |
| Archer (Vagabond, **crossbow**) | 10 | 18 | 14 | 10 | 10 | 10 |
| Wizard | 10 | 12 | 12 | 14 | 10 | 16 |
| Cleric | 12 | 10 | 14 | 16 | 10 | 10 |

> The "balanced everything" all-12 build is a **trap** — it's mediocre at L1 and
> loses by L5. Steer creation toward a clear primary.

---

## 5. Model caveats (read before quoting a win-rate)

The sims are a **stand-and-trade DPS race** and deliberately omit some live
behaviour. They are authoritative for *throughput* and *accuracy*; treat
absolute win-rates as upper/lower bounds, not promises.

- **No movement / kiting.** Ranged creatures (Archer Goblin, disengage 14 tiles)
  can in practice avoid melee entirely; the model fights them toe-to-toe.
- **No regen, potions, bandages, or buffs mid-fight.** Real survivability is
  higher. The Cleric's heals and the Wizard's burst are analysed *separately*
  (§2.5) — their melee-duel rows are a worst-case fallback, not their real play.
- **Creature HP uses the expected roll** (mean of the `hp:` expression), not a
  per-fight re-roll.
- **Levels 9–20 creatures are designed fillers** (`source=designed` in
  `creatures.csv`) to probe the scaling; shipping content currently stops at the
  Cyclops (L8). Claims about L1–L8 are grounded in real creatures.
- DoT (burn/chill/poison) is approximated as a flat estimate; the exact tick
  cadence is not ported.

---

## 6. Reproduce / appendix

```bash
cd tools/balance
python3 run_scenarios.py --selftest   # asserts the port matches the Rust unit tests
python3 run_scenarios.py              # regenerates the four files in docs/balance/
```

| file | contents |
|---|---|
| [`adventurers.csv`](adventurers.csv) | every PC build × {L1,5,10,20}: attrs, HP/mana, to-hit, damage, defence |
| [`creatures.csv`](creatures.csv) | every creature (game + designed): stat block, to-hit, damage, XP |
| [`matchups.csv`](matchups.csv) | closed-form duel for all 612 PC×creature combos + MC for the level-appropriate set |
| [`generated_tables.md`](generated_tables.md) | the nine sweep tables (accuracy, asymmetry, armor, block, weapons, magic, progression, XP, headline duels) |

**Hand-check (auditable).** L1 Generalist (STR 12 → +1) vs Goblin (AGI 11 →
`ability_mod = 0` → dodge DC 10): needs `d20 ≥ 9` → 12/20 = **60%**, matching
`matchups.csv`. A true STR-10 attacker (+0) needs `d20 ≥ 10` = **55%**. STR Fighter
(STR 16 → +3) vs Rat (DC 11) needs `d20 ≥ 8` = **65%**, also matching. The closed-
form hit-rate and the 40k-sample Monte-Carlo agree to within 1% in the selftest.
