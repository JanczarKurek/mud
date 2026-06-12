# World Module Authoring Guide

This is a **self-contained brief**. Hand the whole file to any LLM (or read it
yourself) to write a *world module* for the game **Mud 2.0** — a readable
design document that a separate tool later compiles into real game content
(NPCs, items, quests, dialog). You do **not** need access to the game's code to
write a good module; everything you need is below.

> If you are an LLM: read the whole guide, then produce **one Markdown module**
> in the exact format described in §5, drawing only on the world, mechanics, and
> content menus given here. Prefer reusing the existing content ids in §4 over
> inventing new ones. Output only the module Markdown — no preamble.

---

## 1. What you are producing

A **module** is a short illustrated-prose "adventure book" for one slice of the
world: a place, the folk and creatures in it, the things to find, and a quest or
two to tie them together. Think *tabletop RPG module / scenario*, not a spec
sheet. It should be a pleasure to read on its own.

Under the prose sits a thin layer of structure (stable `id`s and optional
`hints` blocks) so the compiler can turn each described thing into game data.
Write generously in prose; add only as much structure as you care to pin down —
anything you omit, the compiler fills in from the tier tables in this guide.

A module compiles into these game-content kinds (and nothing else for now):
**NPCs** (townsfolk, merchants, creatures), **items** (gear, consumables,
scrolls), **quests** (with their dialog), **spells**, and **recipes**.
**Maps/geography are *not* generated** — locations are prose only, for the human
to build later. New NPCs/items become defined and spawnable; a designer places
them in the world by hand afterward.

The compiler writes everything into a single self-contained folder named after
the module id — `assets/modules/<module-id>/` — which the game loads
automatically. So pick a `module-id` that is unique and stable; it names the
folder. (You only write the prose `.md`; the `build-module` tool produces the
folder.)

---

## 2. The world

A patchwork of hedgerow hamlets, ferry-towns, and forgotten waystones across an
endless wilderness. The roads are old; the hedges remember. **Beastfolk** of
every shape — mice, foxes, badgers, otters, magpies, frogs, snails — keep their
cottages, mind their gardens, brew their tea, and try not to think too much
about what lives in the deeper woods.

Adventurers are oddballs and outliers: the kind of beast who hears a rumor and
*goes* — to clear giant beetles from a turnip cellar, escort a beekeeper home
before dusk, recover a stolen reliquary from a redcap warren, chase a singing
light across a marsh. Magic is real but rare. A scholar-mouse who can throw a
spark is *somebody*; a hedge-priest who can mend bone is *somebody important*.

**Tone: whimsical-but-dangerous.** Cottages with smoking chimneys; iron-bound
cellar doors; songs around fires; teeth in the dark. **Never grimdark.** The
world should feel like a place worth saving, not merely surviving. Names are
cosy and old-English-pastoral (Lockwood Pond, Crow's Tor, Hedge Lane, Whisper
Wood). Ancestry is pure flavor — anyone can be any creature; it has no
mechanical effect.

---

## 3. Mechanics you must respect

Content must fit these rules so it stays balanced and on-theme.

**Attributes (6):** Strength (STR), Agility (AGI), Constitution (CON), Willpower
(WIL), Charisma (CHA), Focus (FOC). Typical mortal scores 8–14; modifier =
`(score − 10) / 2`.

**Classes (4):** Fighter (martial, d10 HP), Wizard (arcane, d4), Cleric (divine
healer/support, d8), Vagabond (rogue/skirmisher, d6). A character is single-class.

**Skills (10) — use these exact names** in `skill_check` and lore gates:
Athletics, Endurance, Perception, Stealth, Thievery, Survival, Spellcraft, Heal,
Lore, Persuasion. (Persuasion governs haggling and talking foes down; Thievery
is locks/traps/pickpocket; Lore identifies things; Heal is first-aid.)

**Tier framework** — every entity belongs to a tier, which sets its power band.
Tag your module and its entities with a tier and keep foes/rewards consistent:

| Tier | Char level | Vibe | Typical foes | Typical reward |
|---|---|---|---|---|
| T1 — Hedgerow | 1–3 | starter | rats, goblins, bandits | copper, common gear |
| T2 — Wildwood | 4–7 | fledgling | wolves, hobgoblins, fey | silver, fine gear |
| T3 — Deep Forest | 8–12 | seasoned | trolls, dark mages, ogres | gold, rare gear |
| T4 — Beyond | 13–20 | legendary | drakes, liches, spirits | gold + named items |

Cross-tier is fine as *intent*, not a wall: a lone T2 fox-bandit can lurk near a
T1 village.

**Combat (for flavor calibration, not for you to compute):** attack = `d20 + BAB
+ ability_mod` vs `AC = 10 + AGI_mod + armor + shield`. Weapon damage is dice
only (e.g. `1d6`); the engine adds the attacker's STR/AGI modifier. So when you
give a weapon `1d8`, that is *before* the wielder's bonus.

**Damage types:** blunt, cut, pierce, fire, frost, earth, lightning, poison,
acid, death, holy, arcane.

**Magic:** spells are class- and level-gated; mana scales with level so costly
spells are naturally high-level. A scroll fires its spell once regardless of class.

**Currency (old-English £sd):** 1 silver = 12 copper; 1 gold = 20 silver
(= 240 copper). Items: `copper_coin`, `silver_coin`, `gold_coin`. Anchors: loaf
1c, ale 6c, dagger 6c, inn night 1s, shortsword 8s, leather armor 1g, chainmail
5g, greater heal potion 15g, named blade 500g+.

---

## 4. Content menus (reuse these ids first)

Before inventing anything, **reuse an existing content id** — the compiler links
to these directly and they already have art and balance. Invent new entities
only when nothing here fits.

**Already-built ids you can reference as foes / rewards / ingredients:**

- Creatures: `rat`, `goblin`, `archer_goblin`, `goblin_mage`, `skeleton`,
  `cyclops`, `fire_elemental`.
- Coins: `copper_coin`, `silver_coin`, `gold_coin`.
- Weapons/ammo: `bronze_sword`, `bow`, `crossbow`, `arrow`, `bolt`,
  `wooden_shield`, `wand_of_sparks`.
- Armor: `leather_helmet`, `leather_armor`, `leather_legs`, `traveler_boots`.
- Trinkets: `copper_amulet`, `silver_ring`.
- Consumables/food: `apple`, `potion`, `poison_flask`, `raw_fish`,
  `cave_mushroom`, `green_herb`, `flowers`, `iron_ore`.
- Scrolls: `lesser_heal_scroll`, `cure_wounds_scroll`, `spark_bolt_scroll`,
  `magic_dart_scroll`, `fireball_scroll`, `frost_lance_scroll`, `bless_scroll`,
  `firewall_scroll`, `flame_weapon_scroll`, `empower_weapon_scroll`,
  `glimmer_scroll`, `light_scroll`, `shield_scroll`, `sleep_scroll`,
  `slow_scroll`, `restore_scroll`.
- Tools/containers/props: `fishing_rod`, `pickaxe`, `herb_knife`, `pen`,
  `canvas_backpack`, `small_pouch`, `herb_pouch`, `barrel`, `iron_chest`,
  `iron_key`, `book`, `torch`, `well`, `sign_post`, `wooden_door`, `lever`,
  `campfire`, `tombstone`, `bear_trap`.

**Calibration tables** — sample what *new* content of each tier should look like.

Enemies (HD = level; HP / damage shown):

| Tier | Examples (HD · HP · damage) |
|---|---|
| T1 | Giant Rat (1·8·1d3), Cave Bat (1·6·1d3), Giant Beetle (1·12·1d4), Kobold (1·8·1d4), Bandit (2·22·1d6), Bramble-mouser cat (3·28·1d6) |
| T2 | Forest Wolf (4·38·1d8), Hobgoblin (4·44·1d8), Redcap Fey (5·45·1d8), Worg (6·60·1d10), Owlbear Cub (7·70·1d10) |
| T3 | Troll (9·100·1d10, regen), Ogre (9·120·2d6), Dark Mage (10·80·spells), Dire Bear (10·130·multi), Ghoul-priest (12·100·1d8+paralysis) |
| T4 | Young Drake (13·180·2d8+fire), Lich-rat (15·150·spells), Ancient Treant (16·240·2d10), Witch-queen of Brambles (20·200·full caster) |

Weapons (damage is dice only):

| Tier | Examples |
|---|---|
| T1 | Dagger 1d4 (throwable), Club/Shortsword/Mace 1d6, Spear 1d8 reach, Sling 1d4 |
| T2 | Scimitar 1d6 (crit 18–20), Longsword 1d8, Battle axe 1d8, Crossbow 1d8 |
| T3 | Rapier 1d6 +2 hit (AGI), Greatsword 2d6, Maul 2d6, Longbow 1d8 |
| T4 | Ash-Heart Spear 1d8+1d6 fire, Glaive of First Frost 1d10+1d6 cold, Starsong Bow 1d8+1d6 lightning |

Armor (AC bonus): cloth +0, leather +1–2, studded/hide +3, chain +4, breastplate
+5, half-plate +6, full plate +7. Shields: buckler +1, round/kite +2, tower +4.

Consumables: minor/standard/greater heal potion (15/35/80 HP), mana potions,
antidote (cures poison), food (small out-of-combat regen), tier-matched scrolls.

Spells (min-level · mana · effect): Magic Dart (1·4·1d4+1 force), Spark Bolt
(1·12·~18), Frost Lance (3·16·2d6 cold + slow), Sleep (3·14·AoE), Fireball
(6·24·5d6 fire AoE); Cleric: Lesser Heal (1·8·20 HP), Bless (1·6·+1 hit), Cure
Wounds (3·14·30 HP), Word of Mending (5·20·party heal).

**Quest-hook seeds** (any makes a good module premise): stolen beekeeper's queen;
giant beetles under the turnip cellar; redcap warren by the western waystone;
something killing geese on Lockwood Pond at night; cottage doors swapped
overnight on Hedge Lane; a hermit on Crow's Tor trading a scroll for a fallen
star; a mouse-scribe wanting a stolen spellbook back from a fox.

---

## 5. The module format

Plain Markdown. Headings carry meaning. Write prose freely; structure is light.

### 5.1 Skeleton

````markdown
# Module: <Title>
<!-- module-id: <snake_case_id> | tier: T1 -->

## Overview
Free prose: the pitch, the hook, the mood, how this slice connects to the
wider world. A few paragraphs.

## Locations
Prose only — no map is generated. Describe each place and what's in it.
### <Place Name> (id: <snake_case>)
Prose...

## NPCs
### <Name> (id: <snake_case>)
Prose: appearance, manner, role, what they want.
```hints
<key: value lines — all optional>
```

## Items
### <Name> (id: <snake_case>)
Prose.
```hints
kind: consumable
```

## Quests
### <Title> (id: <snake_case>)
Prose: who gives it, the objective, the flow, the payoff.
```hints
giver: <npc_id>
kind: fetch
```
````

Optional extra sections: `## Spells`, `## Recipes`. You may also write a
`## Dialog` section with sample lines per NPC; otherwise the compiler writes
serviceable dialog from the NPC + quest prose.

### 5.2 The two conventions that matter

1. **Entity headings carry an id.** Any `###` heading whose text ends with
   `(id: some_snake_case)` is a compilable entity. The id is how everything
   cross-references (a quest's `giver`, a loot drop, a reward). Ids are
   lowercase `snake_case` and **need only be unique within your module** — the
   compiler namespaces them under the module, so two modules can both define a
   `cellar_rat` without clashing.
2. **`hints` is optional and partial.** A fenced ```hints``` block right under an
   entity heading pins mechanical facts. Provide as many or as few keys as you
   like; the compiler infers the rest from the entity's tier. Values are simple
   `key: value` lines (YAML-ish). Lists use `[a, b]`; quantities use `x` (`apple
   x3`); drop chance uses `@` (`bow x1 @0.1`).

### 5.2.1 Referencing ids (scoping)

Whenever a hint names another entity (`giver`, `reward`, `drops`, `objective`
items, `shop`, recipe inputs/outputs, a scroll's `spell`), the name is resolved
in this order:

- **`some_id`** (a bare name) — **this module first, then core.** If your module
  defines `some_id`, it means *your* entity; otherwise it falls back to the core
  game content (the `assets/` catalogue in §4). So `potion`, `copper_coin`, `rat`
  Just Work as references to core content, and `moonshade_grain` refers to the
  one you defined in the module. **This is what you'll use almost always.**
- **`@@/some_id`** — **force core.** Use only when your module defines an id that
  *shadows* a core one and you specifically want the core version (e.g. you made
  your own `potion` but want to also hand out the vanilla `@@/potion`).
- **`other_module/some_id`** — **another module's entity.** A cross-module
  dependency: it resolves to that module's content (which must be installed).

You don't write the namespaced form for your own entities — the compiler adds
the `module/` prefix when it emits files. You just reference them by their bare
local id.

### 5.3 Hints reference

**NPC hints** (all optional):

| key | meaning |
|---|---|
| `tier` | T1–T4; sets default level, stats, HP, damage |
| `level` | explicit creature level (overrides tier default) |
| `hostile` | `true` = attacks on sight; `false` = peaceful (default false) |
| `role` | `townsfolk` / `merchant` / `questgiver` / `creature` (flavor + behavior) |
| `gives_quest` | quest id this NPC offers (auto-wires dialog ↔ quest) |
| `dialog` | `true`/`false` — force a dialog node (implied `true` if it talks or gives a quest) |
| `stats` | partial overrides, e.g. `{ str: 12, foc: 14 }` |
| `attack` | `melee` / `ranged` |
| `damage` | dice only, e.g. `1d6` (engine adds the stat mod) |
| `damage_type` | one of the §3 damage types |
| `range` | tiles, for ranged attackers |
| `hp` | explicit HP dice expression (else derived from tier/level) |
| `drops` | loot list, e.g. `[copper_coin x1d4, cheese_wedge x1 @0.3]` |
| `behavior` | `aggressive` / `skittish` / `{ detect: 8, disengage: 14, step: 1.0 }` |
| `shop` | merchant wares, e.g. `[apple @4, bronze_sword @720 x2]` (`@price_copper`, optional `xstock`) |
| `appearance` | one line of art direction for sprite generation |

Every NPC — even a peaceful, stationary questgiver or shopkeeper — compiles into
a *complete* NPC with stats and behavior defaults, so it's talkable and
targetable wherever it's placed. `hostile: true` makes a creature chase and
attack; `hostile: false` (the default) stands its ground and just talks/trades.

**Item hints:**

| key | meaning |
|---|---|
| `kind` | `equipment` / `consumable` / `scroll` / `pickup` / `pouch` |
| `tier` | sets defaults |
| `slot` | equipment slot: `weapon`/`armor`/`helmet`/`legs`/`shield`/`ring`/`amulet`/`boots`/`backpack` |
| `weight` | kg (default by kind) |
| `damage`, `damage_type` | for weapons |
| `stats` | equip bonuses, e.g. `{ con: 2 }` |
| `effect` | consumable effect, e.g. `{ restore_health: 35 }` or `{ regen_multiplier: 2.0, regen_duration_seconds: 60 }` |
| `spell` | spell id a scroll casts |
| `stack` | max stack size |
| `appearance` | art-direction line |

**Quest hints:**

| key | meaning |
|---|---|
| `giver` | NPC id who offers it (required) |
| `kind` | `fetch` (bring N of X) / `kill` (slay N of type) / `talk` (visit someone) |
| `objective` | one line, e.g. `bring 3 cave_mushroom to old_maple` or `kill 3 rat` |
| `reward` | item list, e.g. `[potion x1, copper_coin x10]` |
| `give_recipe` | recipe id taught on completion (optional) |
| `persuade_bonus` | extra reward unlocked by a Persuasion check (optional), e.g. `[gold_coin x5]` |

> Quest mechanics: **fetch** and **talk** quests need no code (pure dialog).
> **kill** quests get a tiny tracker script. You don't write either — describe
> the quest in prose + hints and the compiler emits the right files.

**Spell hints** (in a `## Spells` section): `incantation`, `mana`, `targeting`
(`targeted`/`untargeted`/`targeted_tile`), `range`, `class` (Wizard/Cleric/…),
`min_level`, `damage`, `damage_type`, `heal`, `aoe_radius`.

**Recipe hints** (in a `## Recipes` section): `inputs` (`[cave_mushroom x3, apple
x1]`), `outputs` (`[potion x1]`), `station` (e.g. `campfire`), `learn`
(`{ class: Cleric, min_level: 1 }`), `xp`.

---

## 6. Worked example

````markdown
# Module: The Sweetwater Mill
<!-- module-id: sweetwater_mill | tier: T1 -->

## Overview
The old grain-mill on the Sweetwater has stood quiet for a year, ever since
Old Maple the miller drowned in the race. The wheel still turns; the villagers
of nearby Lockwood swear they hear grinding at night. Lately the cellar has
filled with bold, fat rats, and a smell of damp flour drifts up the lane.
Someone brave enough to clear the cellar might also lay a gentle ghost to rest.

## Locations
### The Sweetwater Mill (id: sweetwater_mill_house) 
A timber mill leaning over its mossy wheel. The ground floor is sacks of
spoiled flour; a ladder drops into a flooded stone cellar. Maple's ghost keeps
to the upper room, where the account-books are still open to the last day.

## NPCs
### Old Maple, the Miller's Ghost (id: old_maple)
A translucent, stooped badger in a flour-dusted apron, fretting over ledgers
that no longer balance. Gentle, sad, and very particular about good grain. He
won't rest until one last sack is properly milled.
```hints
tier: T1
hostile: false
role: questgiver
gives_quest: last_grain
appearance: translucent pale-blue badger in a dusty miller's apron
```

### Cellar Rat (id: cellar_rat)
Fat, bold rats grown sleek on spoiled flour. They no longer fear a lantern.
```hints
tier: T1
hostile: true
damage: 1d3
drops: [copper_coin x1d4]
behavior: aggressive
appearance: large grey rat with a flour-white muzzle
```

## Items
### Sack of Moonshade Grain (id: moonshade_grain)
A burlap sack of pale, faintly glowing grain — the last harvest Maple never
got to mill.
```hints
kind: pickup
tier: T1
appearance: burlap sack spilling faintly luminous pale grain
```

## Quests
### The Last Grain (id: last_grain)
Old Maple asks the adventurer to clear the rats from his cellar, then bring him
the Sack of Moonshade Grain so he can mill it one final time and rest. Reward:
a healing potion, and his gratitude.
```hints
giver: old_maple
kind: fetch
objective: bring 1 moonshade_grain to old_maple
reward: [potion x1, copper_coin x20]
persuade_bonus: [silver_coin x2]
```
````

---

## 7. Rules of thumb

- **Reuse before inventing.** If `rat` and `potion` fit, use them — don't make
  `cellar_rat` and `healing_draught` for no reason. (The example invents
  `cellar_rat` only to show how a new creature is declared.)
- **Tag a tier** on the module and on new entities. Keep foe HP/damage and
  reward value inside the tier band (§3, §4).
- **Keep questlines to fetch / kill / talk.** That's what compiles cleanly today.
- **Every referenced id must exist** — either reused from §4 or declared as an
  entity in this module. Don't reference a reward or foe you never define.
- **Write the prose first**, then sprinkle hints. A module with rich prose and no
  hints still compiles; the tier defaults carry it.
- **Stay in tone.** Cosy, pastoral, wry, a little eerie. No grimdark, no modern
  idiom, no gore for its own sake.
