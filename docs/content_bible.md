# Content Bible

A *short game book* for Mud 2.0 — generic content tables that fit the mechanics
designed in `docs/progression.md`. Read it once to get the vibe, then rewrite
freely. Numbers here are deliberately first-pass; everything tagged
`[tunable]` in `progression.md §10` is also tunable here.

This doc is **design fuel** — no implementation hangs off it directly. Once
Phase B/C/E from `progression.md §9` lands, the YAML in `assets/` can be
written straight from these tables.

> **Schema note.** Existing creature/weapon YAML uses the older damage form
> `"1d6+strength/5"` (raw attribute, no modifier). The new design in
> `progression.md §7.3` uses `roll(weapon_damage_expr) + STR_mod` — the engine
> adds the modifier. **All damage expressions in this doc use the new form**
> (just dice, e.g. `1d6`). Migration of existing YAML is a Phase B chore.

---

## 1. The setting (one pager)

A patchwork of hedgerow hamlets, ferry-towns, and forgotten waystones laid
across an endless wilderness. The roads are old. The hedges remember. Beastfolk
of every shape — mice, foxes, badgers, otters, magpies, frogs, snails — keep
their cottages, mind their gardens, brew their tea, and try not to think too
much about what lives in the deeper woods.

Adventurers are oddballs and outliers: the kind of beast who hears a rumor and
*goes* — to clear a den of giant beetles from a turnip cellar, escort a
beekeeper home before dusk, recover a stolen reliquary from a redcap warren,
or chase a singing light across a marsh. Magic is real but rare. A scholar
mouse who can throw a spark is *somebody*. A hedge-priest who can mend bone
is *somebody important*.

**Tone.** Whimsical-but-dangerous. Cottages with smoking chimneys; iron-bound
cellar doors; songs around fires; teeth in the dark. Never grimdark. The world
should feel like a place worth saving, not a place worth surviving.

---

## 2. Ancestries

**Anyone can be anyone.** "Beastfolk" covers any small-to-medium critter; ancestry
has **no mechanical effect** — class is the entire build axis (`progression.md
§3`). Pick a creature for flavor. Suggested archetypes:

- mouse scholar / scribe
- fox vagabond / pickpocket
- badger smith / soldier
- otter ranger / ferryman
- rabbit acolyte / forager
- magpie thief / messenger
- hedgehog hermit / herbalist
- frog hedge-mage / fisher
- toad warlock / undertaker
- squirrel scout / acrobat
- weasel duelist / spy
- mole digger / engineer

(Furry, talking, cosy, capable. No size category yet — assume "person-shaped"
even if the model is a mouse.)

---

## 3. Tier framework

Every later table tags content by tier. Tiers map to character level brackets.

| Tier | Level range | Vibe | Typical foes | Typical reward |
|---|---|---|---|---|
| T1 — Hedgerow | 1–3 | starter | rats, goblins, bandits | copper, common gear |
| T2 — Wildwood | 4–7 | fledgling | wolves, hobgoblins, fey | silver, fine gear |
| T3 — Deep Forest | 8–12 | seasoned | trolls, dark mages, ogres | gold, rare gear |
| T4 — Beyond | 13–20 | legendary | drakes, liches, primal spirits | gold + named items |

Cross-tier monsters happen — a **T2 fox bandit** can lurk near a T1 village,
and a **T3 troll** is a credible boss for a T2 party with help. Tier is
*intent*, not a wall.

---

## 4. Weapons

Damage expressions are dice only — `STR_mod` (or `AGI_mod` for ranged)
is added by the combat system per `progression.md §7.3`. Two-handed weapons
get `STR_mod × 1.5`.

Existing assets called out: **bronze_sword**, **bow** (shortbow),
**crossbow**, **arrow**, **bolt**.

### 4.1 Light (1H, off-hand or dual)

| Name | Damage | Weight | Tier | Flavor |
|---|---|---|---|---|
| Dagger | 1d4 | 0.5 | T1 | Whittling-knife with a wrist loop. Throwable. |
| Hand axe | 1d4 | 1 | T1 | A short hatchet, bound with twine. |
| Sickle | 1d4 | 1 | T1 | A reaper's sickle, just sharp enough. |
| Throwing knives (×3) | 1d3 | 0.3 | T1 | Set of three balanced knives in a wrist sheath. |
| Stiletto | 1d3, +1 to-hit vs unarmored | 0.3 | T2 | A thin needle of steel; pierces leather. |

### 4.2 One-handed (martial)

| Name | Damage | Weight | Tier | Flavor |
|---|---|---|---|---|
| Club | 1d6 | 2 | T1 | A knot of oak. |
| Shortsword | 1d6 | 2 | T1 | The standard adventurer's blade. |
| **Bronze Sword** *(existing)* | 1d6 | 2 | T1 | A simple bronze sword with a broad beginner blade. |
| Mace | 1d6 | 3 | T1 | A bronze-headed bludgeon, good against bone. |
| Scimitar | 1d6, crit on 18-20 | 2 | T2 | A curved fox-style blade; dancing footwork required. |
| Longsword | 1d8 | 3 | T2 | A proper soldier's sword. |
| Warhammer | 1d8 | 4 | T2 | A short-handled hammer, head and spike. |
| Battle axe | 1d8 | 4 | T2 | Single-bitted, with a beard for hooking shields. |
| Rapier | 1d6, +2 to-hit | 1.5 | T3 | Duellist's choice; uses AGI for damage. |
| **Ash-Heart Spear** *(named, T4)* | 1d8 + 1d6 fire | 3 | T4 | A spear cut from the burnt heart of an old ash. Sings. |

### 4.3 Two-handed

| Name | Damage | Weight | Tier | Flavor |
|---|---|---|---|---|
| Quarterstaff | 1d6 / 1d6 (double) | 4 | T1 | Standard wandering-friar weapon. |
| Spear | 1d8, reach | 3 | T1 | A long pole with an iron head. |
| Greatclub | 1d10 | 8 | T1 | A whole tree limb, more or less. |
| Halberd | 1d10 | 8 | T2 | A spear that learned to chop. |
| Greataxe | 1d12 | 10 | T2 | Two-handed, no second chances. |
| Greatsword | 2d6 | 8 | T3 | Reserved for the tallest badgers. |
| Maul | 2d6 | 10 | T3 | A blacksmith's hammer made for war. |
| Glaive of First Frost *(named, T4)* | 1d10 + 1d6 cold | 8 | T4 | A pole-arm forged from the icicle that started winter. |

### 4.4 Ranged

| Name | Damage | Range | Ammo | Tier | Flavor |
|---|---|---|---|---|---|
| Sling | 1d4 | 4 | stones | T1 | Free ammo, cheap noise. |
| **Shortbow** *(existing)* | 1d6 | 5 | arrow | T1 | A sturdy shortbow strung with gut. |
| Hand crossbow | 1d4 | 4 | bolt | T2 | Single-handed; loud. |
| **Crossbow** *(existing)* | 1d8 | 6 | bolt | T2 | Heavy, slow, punches through hide. |
| Longbow | 1d8 | 8 | arrow | T3 | A tall bow for long fields. |
| Heavy crossbow | 1d10 | 7 | bolt | T3 | A windlass crossbow; reload is its own turn. |
| **Starsong Bow** *(named, T4)* | 1d8 + 1d6 lightning | 10 | arrow | T4 | Strung with comet-hair. Arrows whistle. |

### 4.5 Thrown

| Name | Damage | Range | Tier | Flavor |
|---|---|---|---|---|
| Dart | 1d3 | 3 | T1 | Bundle of three; cheap and fast. |
| Throwing knife (single) | 1d3 | 3 | T1 | See §4.1 set. |
| Javelin | 1d6 | 4 | T2 | A throwing-spear; usable as a bad melee spear. |
| Hatchet (thrown) | 1d6 | 3 | T2 | The hand axe doubles up. |

---

## 5. Armor & shields

AC values follow `progression.md §7.2`: `AC = 10 + AGI_mod + armor + shield +
dodge`. Numbers below are `[tunable]`. Heavier armor implies an AGI cap
(soft rule — flagged in flavor, not enforced yet).

Existing assets called out: **leather_helmet**, **leather_armor**,
**leather_legs**.

### 5.1 Head

| Name | AC | Tier | Flavor |
|---|---|---|---|
| Cloth hood | +0 | T1 | Hood and ear-holes. Mostly to keep rain off. |
| Leather cap | +1 | T1 | A simple skullcap. |
| **Leather Helmet** *(existing)* | +1 | T1 | A studded leather cap. |
| Mail coif | +1 | T2 | Chainmail hood; muffles hearing. |
| Steel cap | +1 | T2 | An open-faced helmet. |
| Great helm | +2 | T3 | Full-face; -2 Perception (vision). |
| Dragon-scale helm | +3 | T4 | Lightweight; whispers when you turn your head. |

### 5.2 Torso

| Name | AC | Tier | Flavor |
|---|---|---|---|
| Padded gambeson | +1 | T1 | Quilted linen, lots of straw. |
| **Leather Armor** *(existing)* | +2 | T1 | A sturdy leather cuirass. |
| Studded leather | +3 | T1 | Leather with bronze rivets. |
| Hide armor | +3 | T2 | Boiled hide of something larger than you. |
| Chainmail shirt | +4 | T2 | Linked rings to mid-thigh. |
| Breastplate | +5 | T3 | Steel plate over the chest. |
| Half-plate | +6 | T3 | Plate over chain over gambeson. |
| Full plate | +7 | T4 | Articulated plate; shines, clanks. |
| Runic plate *(named, T4)* | +8 | T4 | Engraved with binding-runes; faintly warm. |

### 5.3 Legs

| Name | AC | Tier | Flavor |
|---|---|---|---|
| Cloth breeches | +0 | T1 | Pants. |
| **Leather Legs** *(existing)* | +1 | T1 | Leather greaves with shin straps. |
| Hide leggings | +2 | T2 | Heavy boiled hide. |
| Mail leggings | +2 | T2 | Chain skirt to the knee. |
| Plate greaves | +3 | T3 | Articulated steel; clatters. |

### 5.4 Shields

| Name | AC | Tier | Flavor |
|---|---|---|---|
| Buckler | +1 | T1 | Strapped to the forearm; doesn't free-block. |
| Round shield | +2 | T1 | A wooden disk with an iron rim. |
| Kite shield | +2 | T2 | Tall and tapered; favored by mounted critters. |
| Heater shield | +2 | T2 | Heraldic-shaped, balanced. |
| Tower shield | +4 | T3 | Slow, wall-like, -2 to-hit while wielded. |
| Mirror Shield *(named, T4)* | +3, reflects rays | T4 | A polished disc; bounces beam spells back. |

---

## 6. Enemy bestiary

Statlines are mechanically valid first-pass numbers. Layout:
- **HD** = monster level (used for saves and XP).
- **AC** computed from natural armor + size; `progression.md §7.2`.
- **BAB** progression: brute = full, normal = 3/4, caster = 1/2.
- **Saves** shown as F/R/W ("+" = good track, "−" = poor track,
  `progression.md §7.4`).
- **XP** = `HD² × 50` per `progression.md §4.2`.
- **Behavior** maps to existing AI tags (`Roaming`, `HostileChase`,
  `RangedKite`) plus a planned `Caster` tag.

### 6.1 T1 — Hedgerow (HD 1–3)

| Name | HD | AC | BAB | F/R/W | Damage | HP | XP | Behavior | Flavor |
|---|---|---|---|---|---|---|---|---|---|
| **Giant Rat** *(existing)* | 1 | 12 | +0 | −/+/− | 1d3 (bite) | 8 | 50 | Roaming | Mangy, oversized, beady-eyed. |
| Cave bat | 1 | 13 | +0 | −/+/− | 1d3 | 6 | 50 | Roaming | Squeaks first, bites second. |
| Giant beetle | 1 | 14 | +0 | +/−/− | 1d4 | 12 | 50 | Roaming | Mandibles like garden shears. |
| Forest sprite (mischief) | 1 | 14 | +0 | −/+/+ | 1d3 | 5 | 50 | Caster | Steals shiny things; throws sparks. |
| Kobold scrabbler | 1 | 12 | +1 | −/+/− | 1d4 (dagger) | 8 | 50 | HostileChase | Yappy, cowardly, dangerous in groups. |
| **Goblin** *(existing)* | 2 | 13 | +1 | −/+/− | 1d4 (knife) | 24 | 200 | HostileChase | Wiry, green, mean stare. |
| **Archer Goblin** *(existing)* | 2 | 13 | +1 | −/+/− | 1d6 (shortbow) | 22 | 200 | RangedKite | Goblin with a bow; runs while shooting. |
| Bandit (foxfolk) | 2 | 14 | +1 | +/+/− | 1d6 (shortsword) | 22 | 200 | HostileChase | Brushtail mask, flask of cheap brandy. |
| **Skeleton** *(existing)* | 3 | 13 | +2 | +/−/− | 1d6 (shortsword) | 33 | 450 | HostileChase | Reanimated, flickering green eye-light. |
| Bramble-mouser cat | 3 | 14 | +2 | −/+/− | 1d6 (claws) | 28 | 450 | HostileChase | A wild house-cat the size of a wolf. |

### 6.2 T2 — Wildwood (HD 4–7)

| Name | HD | AC | BAB | F/R/W | Damage | HP | XP | Behavior | Flavor |
|---|---|---|---|---|---|---|---|---|---|
| Forest wolf | 4 | 14 | +3 | +/+/− | 1d8 (bite) | 38 | 800 | HostileChase | Trip on hit; hunts in pairs. |
| Hobgoblin | 4 | 16 | +4 | +/−/− | 1d8 (longsword) | 44 | 800 | HostileChase | Disciplined, drilled, banner-proud. |
| Hobgoblin archer | 4 | 15 | +4 | +/−/− | 1d8 (longbow) | 36 | 800 | RangedKite | Sets up overwatch, retreats deliberately. |
| Redcap fey | 5 | 16 | +3 | −/+/+ | 1d8 (sickle) | 45 | 1250 | HostileChase | Murderous, blood-stained; vulnerable to iron. |
| Brigand-mage | 5 | 13 | +2 | −/−/+ | spark bolt + dagger | 30 | 1250 | Caster | Foxfolk hedge-wizard turned road-thief. |
| Swarm of crows | 5 | 15 | +3 | −/+/− | 2d6 (swarm) | 40 | 1250 | HostileChase | Cannot be flanked; immune to single-target spells. |
| Worg | 6 | 16 | +4 | +/+/− | 1d10 (bite) | 60 | 1800 | HostileChase | Wolf the size of a pony, smarter. |
| Owlbear cub | 7 | 17 | +5 | +/−/− | 1d10 (claw/claw/bite) | 70 | 2450 | HostileChase | Half owl, half bear, all furious. |

### 6.3 T3 — Deep Forest (HD 8–12)

| Name | HD | AC | BAB | F/R/W | Damage | HP | XP | Behavior | Flavor |
|---|---|---|---|---|---|---|---|---|---|
| **Cyclops** *(existing)* | 8 | 16 | +6 | +/−/− | 1d12 (greatclub) | 110 | 3200 | HostileChase | Hulking, one-eyed, short temper. |
| Troll | 9 | 16 | +6 | +/−/− | 1d10 (claw/claw) | 100 | 4050 | HostileChase | Regenerates 5/turn; fire stops it. |
| Ogre | 9 | 17 | +6 | +/−/− | 2d6 (greatclub) | 120 | 4050 | HostileChase | Slow, dumb, devastating on a connect. |
| Dark mage (anyfolk) | 10 | 15 | +5 | +/−/+ | fireball + frost lance | 80 | 5000 | Caster | Tower-trained, has gone to seed in the wild. |
| Dire bear | 10 | 17 | +7 | +/−/− | 1d10+1d10+1d8 | 130 | 5000 | HostileChase | Three attacks per turn (claw/claw/bite). |
| Will-o-wisp | 11 | 24 | +5 | +/+/+ | 2d8 (electric) | 60 | 6050 | RangedKite | Ethereal; only magic touches it. |
| Ghoul-priest | 12 | 18 | +6 | +/−/+ | 1d8 + paralysis | 100 | 7200 | Caster | Was a hedge-priest, then wasn't. |

### 6.4 T4 — Beyond (HD 13–20)

| Name | HD | AC | BAB | F/R/W | Damage | HP | XP | Behavior | Flavor |
|---|---|---|---|---|---|---|---|---|---|
| Young drake | 13 | 20 | +9 | +/+/− | 2d8 + 2d6 fire breath | 180 | 8450 | HostileChase | Wing-coverts the size of doors. |
| Lich-rat | 15 | 19 | +7 | +/+/+ | finger of death + 1d8 | 150 | 11250 | Caster | Once a wizard mouse. Now neither. |
| Ancient treant | 16 | 22 | +12 | +/−/+ | 2d10 (slam) ×2 | 240 | 12800 | HostileChase | Cannot be moved; takes only what it gives. |
| Primal stag-spirit | 18 | 21 | +13 | +/+/+ | 2d8 (gore) + trample | 220 | 16200 | HostileChase | Antlers like a cathedral. Holy or fey, no one's sure. |
| Witch-queen of brambles | 20 | 22 | +10 | +/+/+ | full spell list | 200 | 20000 | Caster | A toad-queen on a thorn-throne. The forest answers her. |

---

## 7. Spells

Schema: `name`, `incantation`, `class_access`, `min_caster_level`, `mana_cost`,
`targeting`, `range_tiles`, `effects` per `progression.md §6` (with the new
`class_access` and `min_caster_level` fields layered onto the existing
`SpellDefinition`).

Mana costs scale with `min_caster_level` so the §6.1 mana growth gates
high-level spells naturally. Wizard-only and Cleric-only lists overlap on
utility spells.

Existing spells called out: **Spark Bolt** (Wizard), **Lesser Heal** (Cleric).

### 7.1 Wizard list

| Name | Min lvl | Mana | Range | Effect | Flavor |
|---|---|---|---|---|---|
| Magic Dart | 1 | 4 | 6 | 1d4+1 force, auto-hits | A point of light, then a sting. |
| Glimmer (light) | 1 | 2 | self/touch | lights 4 tiles for 10 min | A drop of sun on a fingertip. |
| **Spark Bolt** *(existing)* | 1 | 12 | 5 | 18 dmg | *Exori Vis.* A crackling jolt. |
| Frost Lance | 3 | 16 | 6 | 2d6 cold + 1-tile slow | A spear of clean ice. |
| Sleep | 3 | 14 | 4 (AoE) | sleeps HD ≤ 4 targets | Wisteria smoke. |
| Shield | 3 | 8 | self | +4 AC, 1 minute | A pane of stillness. |
| Slow | 5 | 18 | 5 | half-move on target, 3 turns | Honey in the joints. |
| Fireball | 6 | 24 | 8 (3-tile AoE) | 5d6 fire | Pop, then everything's hot. |
| Counterspell | 7 | 20 | 6 | cancels target spell | A backwards-said word. |

### 7.2 Cleric list

| Name | Min lvl | Mana | Range | Effect | Flavor |
|---|---|---|---|---|---|
| **Lesser Heal** *(existing)* | 1 | 8 | self | restore 20 HP | *Exura.* Warmth in the bones. |
| Bless | 1 | 6 | 4 (party) | +1 to-hit, 1 minute | The day brightens. |
| Cure Wounds | 3 | 14 | touch | restore 30 HP | Whispered name of the saint. |
| Sanctuary | 3 | 10 | self | foes pass Will save to attack you | A quiet circle of grass. |
| Smite | 4 | 16 | melee | next attack +2d6 holy | The blade hums. |
| Cure Disease | 5 | 18 | touch | removes disease/poison | Bitter herbs and a song. |
| Word of Mending | 5 | 20 | 4 (party) | 4d6 group heal | A song the wind picks up. |
| Restore | 8 | 28 | touch | full HP, removes status | Two hands and an old prayer. |
| Resurrection | 13 | 60 | corpse | restore the dead | An entire grove goes quiet. |

### 7.3 Shared (any caster)

| Name | Min lvl | Mana | Range | Effect | Flavor |
|---|---|---|---|---|---|
| Detect Magic | 1 | 4 | self (8-tile aura) | reveals magic items/auras | Ears prickle around the strange. |
| Mend | 1 | 4 | touch | repairs small object | Like darning a sock. |
| Light | 1 | 2 | object | object glows, 30 min | A coin, a stick, a lantern. |

---

## 8. Consumables

Existing assets called out: **apple**, **potion**, **lesser_heal_scroll**,
**spark_bolt_scroll**, **copper_amulet**, **silver_ring**.

### 8.1 Potions

| Name | Tier | Effect | Flavor |
|---|---|---|---|
| Minor heal potion | T1 | restore 15 HP | A red-tinged tincture in a tiny bottle. |
| Standard heal potion | T2 | restore 35 HP | A proper apothecary bottle, waxed. |
| Greater heal potion | T3 | restore 80 HP | Heavy as a stone, glows faintly. |
| Minor mana potion | T1 | restore 10 mana | A cool blue cordial. |
| **Mana Potion** *(existing — generic)* | T2 | regen ×2 for 60s | Tingles as your mana returns. |
| Greater mana potion | T3 | restore 40 mana | Tastes like rain. |
| Antidote | T1 | cures poison | Smells of garlic and bog-myrtle. |
| Stoneskin draught | T3 | +3 AC, 5 min | Chalky aftertaste; teeth feel heavy. |
| Swiftness elixir | T2 | move +1 tile/turn, 1 min | Carbonated; mildly alarming. |

### 8.2 Food (out-of-combat HP regen tick)

| Name | Tier | Effect | Flavor |
|---|---|---|---|
| **Apple** *(existing)* | T1 | small heal / hunger tick | A crisp apple. |
| Bread loaf | T1 | small heal / hunger tick | Two days old; still good. |
| Cheese wedge | T1 | small heal | A pungent farmhouse round. |
| Dried fish | T2 | medium heal | River trout, smoked. |
| Traveler's stew | T2 | regen tick for 2 min | In a tin; eat by a fire. |
| Honeycake | T3 | medium heal + buff +1 morale | Stamped with the village seal. |

### 8.3 Scrolls (one-shot)

A scroll fires its spell once, ignoring class_access. Use a scroll's tier as
its spell's tier.

| Scroll | Spell | Tier |
|---|---|---|
| **Lesser Heal Scroll** *(existing)* | Lesser Heal | T1 |
| **Spark Bolt Scroll** *(existing)* | Spark Bolt | T1 |
| Magic Dart Scroll | Magic Dart | T1 |
| Frost Lance Scroll | Frost Lance | T2 |
| Cure Wounds Scroll | Cure Wounds | T2 |
| Fireball Scroll | Fireball | T3 |
| Resurrection Scroll | Resurrection | T4 |

### 8.4 Trinkets (equipment slots — amulet / ring / charm)

| Name | Slot | Stats | Tier | Flavor |
|---|---|---|---|---|
| **Copper Amulet** *(existing)* | amulet | +2 CON | T1 | A cheap amulet on a braided cord. |
| **Silver Ring** *(existing)* | ring | +2 WIL, +1 FOC | T2 | A plain silver band that catches the light. |
| Iron ring | ring | +1 STR | T1 | Cold to the touch. |
| Feather charm | amulet | +2 Stealth | T1 | A single grey feather on a cord. |
| Acolyte's pendant | amulet | +1 WIL, +5 max mana | T2 | Stamped with the village saint. |
| Ring of fox-step | ring | +2 AGI | T3 | Glints like a fox's eye. |
| Cloak-pin of warding | amulet | +1 to all saves | T3 | A bronze rune-pin. |
| Crown of the Hedge-King | amulet | +3 CHA, +1d6 mana regen/min | T4 | Said to grow on you. Possibly literally. |

---

## 9. Currency & economy

Three coin tiers, **old-English £sd**: **1 silver = 12 copper**,
**1 gold = 20 silver** (so 1 gold = 240 copper). Drop tables and prices use
copper/silver/gold inline (e.g. *"50c"* or *"3g 4s 6c"*). The
`assets/overworld_objects/{copper,silver,gold}_coin/` items each define
12 stack-tier sprites covering 1, 2, 3 ... 9, then 10–19, 20–49, 50–100.

The conversion arithmetic lives in `src/game/currency.rs`:
`COPPER_PER_SILVER = 12`, `SILVER_PER_GOLD = 20`,
`COPPER_PER_GOLD = 240`, plus `total_copper(c, s, g)` and
`split(copper) -> (g, s, c)` helpers.

### 9.1 Price ladder (anchors)

| Item | Price | Copper-equivalent | Notes |
|---|---|---|---|
| Loaf of bread | 1c | 1 | One day's calories. |
| Torch | 2c | 2 | Burns 1 hour. |
| Apple | 3c | 3 | Nicer than the bread. |
| Dagger | 6c | 6 | Common as dirt. |
| Mug of ale | 6c | 6 | Tavern price. |
| Inn room (1 night) | 1s | 12 | With breakfast. |
| Shortsword | 8s | 96 | Standard adventurer kit. |
| Leather armor | 1g | 240 | A blacksmith's commission. |
| Longsword | 1g 8s | 336 | A real soldier's blade. |
| Chainmail | 5g | 1200 | Two months for a smith. |
| Longbow | 8g | 1920 | Including a quiver. |
| Greater heal potion | 15g | 3600 | Apothecary special order. |
| Plate armor | 60g | 14400 | Made for one specific beastfolk. |
| Named legendary blade | 500g+ | 120000+ | Or unsellable. |

### 9.2 Vendor convention (when vendors land)

- Buy at price, sell at half. Persuasion modifies up to ±20% per
  `progression.md §5`.
- Some named items refuse to be sold (story-only).

---

## 10. Loot tiers

A *generic schema* for what an encounter or chest drops. Real loot tables
implement this in code/YAML later.

| Tier | Coin roll | Consumable roll (50%) | Equipment roll (10%) | Notes |
|---|---|---|---|---|
| T1 | 1d10 copper | T1 potion / scroll / food | T1 weapon or armor piece | Hedgerow chest. |
| T2 | 1d10 silver | T2 potion / scroll | T2 weapon or armor piece | Wildwood cache. |
| T3 | 1d6 gold | T3 potion / scroll | T3 weapon or armor piece | Deep-forest hoard. |
| T4 | 2d10 gold + named item | T3-T4 scroll | T4 named item (5%) | Beyond — one-of-a-kind. |

Per-monster drops follow the existing `loot:` schema in
`assets/overworld_objects/*/metadata.yaml` (probability + quantity per type_id).
The tier here is a tuning guideline.

---

## 11. Quest hooks / rumor table

Tavern-rumor one-liners. Use any of these as a starter quest, a Yarn dialog
seed, or a piece of overheard chatter. Tagged by archetype.

### 11.1 Fetch

1. A beekeeper's prize queen has been stolen — tracks lead into the bramble.
2. The miller's lucky hat ended up in the river; he won't grind without it.
3. An old recipe calls for "moonshade mushroom" — only grows in the cellar of the abandoned chapel.
4. The schoolmaster needs three copies of the *Rabbit's Almanac* recovered from a flooded scriptorium.

### 11.2 Clear

5. Giant beetles have made a den under the turnip cellar. The mayor will pay per shell.
6. A redcap warren has set up by the western waystone — too close to the road.
7. Something is killing the geese on Lockwood Pond at night; pawprints are wrong.
8. The old smithy is occupied by skeletons that walk in a circle and won't stop. Annoying. Possibly cursed.

### 11.3 Escort

9. A merchant convoy needs a guard for the four-day trip to the next ferry-town. Bandits on the road.
10. A frail badger pilgrim must reach the standing stones before the equinox.
11. A bride and her dowry-cart need protecting; there is a jilted suitor with hired knives.
12. A tax collector needs an escort *out* of an unfriendly hamlet.

### 11.4 Mystery

13. Cottages on Hedge Lane wake to find their doors swapped overnight. No one has lost anything else.
14. A child swears a tin soldier is whispering at her from the chimney.
15. The well-water in the next valley has gone briefly sweet, then briefly bitter, three days running.
16. A fox came back from the forest speaking only nouns. He used to be a poet.

### 11.5 Wilderness

17. A trapper hasn't checked his line in a week; his cabin is intact but his ax is gone.
18. An old hermit on Crow's Tor will trade a rare scroll for a star-fallen stone — the meteor lit up the sky last week.
19. Survey the path through Whisper Wood and report what's blocking it; nobody's come back through in a month.
20. A river otter is recruiting volunteers to retrieve a sunken bell — old shrine, freshwater, possibly haunted.

### 11.6 Personal

21. A mouse scribe wants their late mentor's spellbook back from the fox who "borrowed" it.
22. A retired soldier wants to bury an old comrade properly — the body is in a ravine the wolves now own.
23. A village priest's faith has slipped; a relic from the old shrine might restore it.
24. A young vagabond wants help robbing a tax-house. Not strictly legal. Pay good.

### 11.7 Bonus

25. A traveling player has lost their entire troupe — they remember a clearing, a song, and waking up alone.

---

## 12. Pouches & carry weight

### 12.1 Pouches

A **pouch** is a small carryable container. Pouches are the only container
items that can themselves sit inside another container — drop a pouch into
your backpack or a chest and it preserves its contents. The rule is
declared in YAML (`assets/object_bases/pouch.yaml`), not Rust:

- pouches inherit `storable: true` from `pickup`
- pouches set `accepts_storable_containers: false` so they reject other
  pouches placed inside them — nesting is capped at depth 1

Initial pouches in `assets/overworld_objects/`:

| Name | Slots | Weight | Tier | Flavor |
|---|---|---|---|---|
| Small Pouch | 4 | 0.5 | T1 | A leather drawstring sack. Holds a few small things. |
| Herb Pouch | 6 | 0.6 | T2 | Linen, divided into compartments. Smells of dried mint. |

A future content batch can add more (money pouch, scroll case, herbalist's
kit) by `extends: pouch` plus a custom sprite.

### 12.2 Carry weight

Items have a `weight` (kg) field; weights for the existing armor/weapon
tables (§4–§5) match the values listed there. The player has two limits,
both derived from STR:

- **Soft cap** = `20 + STR × 2 kg` — over this, the player is `Encumbered`
  and walks at half speed. The Status panel shows
  *"Weight: 25.4 / 20 kg (Encumbered)"* in red.
- **Hard cap** = `soft × 1.5` — pickups above this are rejected outright
  with *"Too heavy — you can't lift that."*

Coins carry a tiny `0.005 kg` each so 200 copper weighs 1 kg — enough that
a beggar's hoard stays light, a dragon's hoard genuinely encumbers.
Pouches contribute their own weight *plus* the recursive sum of their
contents (you can't sneak around the cap by stuffing a backpack with
pouches).

Tunables: see `MaxCarryWeight::from_strength` in
`src/player/components.rs`. Mark anything load-bearing here as
`[tunable]` per `progression.md §10`.

---

## See also

- `docs/progression.md` — the mechanical layer this content sits on top of (classes, leveling, combat, magic).
- `docs/yaml_formats.md` — schema reference for `assets/overworld_objects/*/metadata.yaml` and `assets/spells/*.yaml`. Tables in this doc fit those schemas.
- `assets/overworld_objects/` — already-implemented assets called out in §4–§8.
- `PLAN.md` §4.2 — Phase 6 of the project roadmap. Implementation gates this content.
- `FEATURE_BACKLOG.md` §1 "Living-world batch" — vendor / quest log / spawner work that turns these tables into actual gameplay.
