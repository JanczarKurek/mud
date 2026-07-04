# Combat & Stats Balance Report

_Generated from a faithful Python re-implementation of the live combat math
(`tools/balance/`). Every number below traces to a quoted formula or a cell in
`adventurers.csv` / `creatures.csv` / `matchups.csv` / `generated_tables.md`._

> **Status.** This report describes the state **after the balance retune**
> (modifier-based damage, level-scaled dodge DC, crits, Backstab, Weapon Focus,
> gear tiers, creature re-budget, XP/mana retunes). The pre-retune diagnosis it
> replaced — flat player offence, NPC auto-hit, tools out-damaging swords,
> burst-only magic — is preserved in git history; the follow-ups still open
> live in `ISSUES.md` → "Combat & progression balance".

---

## TL;DR

The whole 1–20 band now plays: same-level fights are **mostly safe** for a
focused build (~85–100% win, with real HP loss), fighting **+3–4 levels up is a
coin flip to a loss** even for the best build, XP pacing is a flat ~13
kills/level, casters sustain a cantrip every ~10 s, and gear/tier/skill choices
all show up in the numbers. Trap builds still exist **on purpose** (tools,
never upgrading) — but they're signals now, not ambushes.

---

## 1. The scaling scheme (what governs every number)

One d20 system, symmetric for players and creatures:

| channel | formula | growth source |
|---|---|---|
| to-hit | `d20 + ability_mod + bab_at(track, level)` (+ Weapon Focus +1+L/5, Fighter melee) | class/creature BAB track |
| dodge DC | `10 + (3·level)/4 + AGI_mod + item dodge` | universal level term |
| weapon damage | `dice + str_mod/agi_mod (+ tier flat) + level/2` | dice = tier, mods = build, level = skill growth |
| crits | raw d20 ≥ weapon `crit_range` (default 20) → roll damage twice | weapon property (dagger 19–20) |
| mitigation | full `armor` subtract (deterministic), full `block` on a `block_chance` gate | gear tiers |
| spell damage | per-spell expression, e.g. `3d6+foc_mod*2+level/2` | Focus modifier + caster level |
| heals / DoTs | expressions too (`2d8+wil_mod*2+level`; burn `1+foc_mod/3+level/6`/tick) | caster-scaled |
| mana regen | `2 + WIL + FOC/2` per minute | attributes |
| XP | `victim_level × 75` vs the `1000·N(N−1)/2` curve | constant ~13 kills/level |

The **to-hit/dodge level terms cancel at level parity**, so same-level hit%
sits in a ~55–85% band at every level (the nat-20/1 rule floors both tails at
5%). Fighting up-level moves *both* channels against you — that, plus the
creature budget below, is what makes +3–4 deadly.

**Creature budget template** (`tools/balance/creatures.py`, mirrored in the
shipping YAML): mean damage ≈ `2 + 1.3·level` as `dice + flat`; mean HP ≈
`20 + 11·level` (tanky ×1.4, fragile ×0.7); armor ≈ `level/3` for armored
types; `bab_track` full for brutes (Cyclops, Ogre Brute), ¾ default.

**Gear progression** (the ~L4–12 upgrade path): vendor sells iron sword,
dagger, longbow, and the chain set; the **Ogre Brute (L10)** drops chain +
iron, the **Dire Wight (L12)** drops steel sword / plate / tower shield.
Weapon tiers move the dice + flat (bronze `1d8` → iron `1d10+1` → steel
`2d6+2`), armor tiers move mitigation (leather 4 → chain 7 → plate mix 9 +
tower shield block 6 @35%).

## 2. The numbers now

Full tables in [`generated_tables.md`](generated_tables.md). Headline
Monte-Carlo win rates vs the level-appropriate creature (2500 trials):

| build | L1 | L5 | L10 | L20 |
|---|---|---|---|---|
| STR Fighter (upgrades at 6/12) | 100% | 100% | 100% | 99% |
| Tank Fighter | 100% | 100% | 100% | 100% |
| Balanced Cleric (self-heals) | 100% | 100% | 100% | 100% |
| Generalist (all-12, late upgrades) | 100% | 100% | 100% | 28% |
| Archer bow / crossbow | 100% | 100% | 100% | 54–60%* |
| Vagabond Skirmisher (dagger) | 100% | 100% | 27%* | 0%* |
| Brute (pickaxe, never upgrades) | 100% | 100% | 2% | 0% |
| Glass Wizard (melee fallback) | 100% | 29%* | 0%* | 0%* |

`*` = **known model floor, not a live-game prediction** (see §3). The un-starred
rows are the calibration targets and they hit the design intent: focused,
gear-current builds are safe at level; the all-12 generalist gets a visible
late-game warning; the tool-wielder is a trap on purpose.

Cross-level (closed-form TTK ratio, `matchups.csv`): STR Fighter L5 beats the
+3 Cyclops (0.64) but loses to the +4 Ogre (1.05); the Generalist already loses
at +3 (1.15); the crossbow Archer loses at +4 (1.36). Level gaps bite.

XP pacing (§8 of the tables): **13–14 kills per level, flat, L1→L19.**

Magic economy (§6): Magic Dart sustains 1.14 DPS on regen alone (a cast every
~9 s), Spark Bolt is the nuke (26.5 mean at L20, one every ~28 s sustained),
the DoT carriers (Frost Bolt, Immolation, Venom) pay out over 8–12 s at the
best paper efficiency, and the AoEs price in their footprint.

## 3. Model caveats (read before quoting a win-rate)

The sims are a **stand-and-trade DPS race**. They are authoritative for
throughput and accuracy; the starred rows above are builds whose real kit the
model deliberately omits:

- **No kiting.** Ranged builds fight toe-to-toe here; live archers open at
  range 5–6 and re-open constantly. Their stand-and-trade rows are a floor.
- **No Backstab.** The Vagabond's opener (+Nd6 from undetected stealth, N =
  1 + level/4 — up to 6d6 ≈ +21) and re-stealth play are outside the duel
  loop; the dagger Skirmisher's rows are its cornered-in-a-hallway worst case.
- **Wizard rows are the melee fallback.** Casters are analysed via the §6
  magic economy, not their sword arm.
- **Cleric self-healing IS modelled** (casts below half HP while mana lasts) —
  that's why its rows are honest.
- Creature HP uses the expected roll; DoTs are flat estimates; L4–5, 7, 9 (ogre
  variant), 14–20 creatures are `source="designed"` fillers probing the curve —
  shipping content now reaches the **Dire Wight (L12)**.

## 4. Deliberate signals & open follow-ups

- **Trap builds are signals**: tools carry no `level` growth term, so a pickaxe
  Brute collapses by L10 — the fix is buying a real weapon, which the vendor
  sells. Consider an in-game hint ("this tool isn't a weapon") in a UX pass.
- **The Ancient Dragon is a standard-budget L20 creature** — true boss variants
  (2–3× budget, mechanics) are future content.
- Remaining follow-ups (tracked in `ISSUES.md`): two-handed ×1.5, damage-type
  resistances, finesse weapons (AGI-to-hit melee), Persuasion/CHA rework,
  Lore/Spellcraft/Heal/Survival utility mechanics (utility_systems.md Slices
  4–5), drop-protection on death, boss variants.

## 5. Reproduce / appendix

```bash
python3 tools/balance/run_scenarios.py --selftest   # asserts the port matches the Rust unit tests
python3 tools/balance/run_scenarios.py              # regenerates the four files in docs/balance
```

| file | contents |
|---|---|
| [`adventurers.csv`](adventurers.csv) | every PC build × {L1,5,10,20}: attrs, HP/mana, to-hit, damage, defence |
| [`creatures.csv`](creatures.csv) | every creature (game + designed): stat block, to-hit, damage, XP |
| [`matchups.csv`](matchups.csv) | closed-form duel for all PC×creature combos + MC for the level-appropriate set |
| [`generated_tables.md`](generated_tables.md) | the nine sweep tables (accuracy, asymmetry, armor, block, weapons, magic, progression, XP, headline duels) |

**Hand-check (auditable).** L1 STR Fighter (STR 16 → +3, full BAB +1, Weapon
Focus +1 → +5) vs Giant Rat (L1, AGI 13 → dodge DC `10 + 0 + 1 = 11`): needs
`d20 ≥ 6` → 75%, matching the headline table. The rat swings back at `−3 + 0`
vs the fighter's DC `10 + 0 + 1 (AGI 12) + 1 (boots) = 12` → needs 15 → 30%. ✓
