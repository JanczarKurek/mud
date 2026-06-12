---
name: build-module
description: Compile a Markdown "world module" into real game content for this project — NPC/item YAML, Yarn dialog, Python quest scripts, spells, and recipes — wiring all cross-references and generating sprites. Use after a module has been drafted (see draft-module) and reviewed.
argument-hint: "<path-to-module.md>"
allowed-tools: Read Write Edit Glob Bash Skill
---

Compile the world module at **$ARGUMENTS** into game content.

A *module* is a prose design doc (format: `docs/modules/AUTHORING_GUIDE.md` §5).
Your job is to turn each described entity into the data files the game loads,
keeping every cross-reference correct. **Mirror the existing exemplar files
exactly** — they are the real schema (the JSON files in `assets/schemas/` are
partial and out of date; do not treat them as authoritative).

**All generated content goes inside one self-contained module folder** —
`assets/modules/<module-id>/` (call it `MODULE_DIR`) — which mirrors the global
asset layout so a whole content pack lives in one place and can be removed by
deleting the folder. Never write into the global `assets/overworld_objects/`,
`assets/dialogs/`, `assets/quests/`, `assets/spells/`, or `assets/recipes/` —
that risks clobbering core content. The game loads `MODULE_DIR` because the
asset loaders overlay `assets/modules/*/<subdir>` onto the global dirs
(`AssetResolver::scan_dirs`, the quest engine, the Yarn folder source, and the
TcpClient asset sync).

```
assets/modules/<module-id>/
  overworld_objects/<id>/metadata.yaml   (+ sprite.png / sheet.png)
  spells/<id>.yaml
  recipes/<id>.yaml
  dialogs/<module-id>.yarn
  quests/<id>.py
  module.md                              (copy of the source module, for provenance)
```

**Ids are scoped to the module.** The engine registers a module's
object/spell/recipe/quest under the **qualified id `<module-id>/<local-id>`**
(it derives this from the folder — you keep the clean local dir name on disk).
So two modules can both define `cellar_rat` with no clash, and you don't need a
globally-unique-name check. Your job is to **resolve every reference to its
absolute id at compile time** and write that absolute id into the emitted files
(the engine never resolves relative refs — it only sees absolute strings).

Resolution rules (apply to every referenced id — `giver`, `reward`, `drops`,
`objective` items, `shop`, recipe I/O, scroll `spell`):

- **bare `some_id`** → if **this module defines `some_id`**, emit
  `<module-id>/some_id`; else emit the **core** id `some_id` (verify it exists in
  `assets/overworld_objects/`, `assets/spells/`, etc.).
- **`@@/some_id`** → force core: emit bare `some_id` (verify it exists in core).
- **`other_mod/some_id`** → another module: emit `other_mod/some_id` verbatim
  (verify `assets/modules/other_mod/...` defines it; warn if absent).

Yarn node titles and dialog variables live in the Yarn runtime's *global*
namespace (not engine-scoped), so keep prefixing them with the module to stay
unique (e.g. `HauntedMillOldMaple`, `$haunted_mill_<quest>_started`).

## Your task

### 1. Read the contract and parse the module

- Read `docs/modules/AUTHORING_GUIDE.md` (§3 mechanics, §4 content ids, §5 format
  + `hints` reference). Then read the module file at the path above.
- Read these exemplars before writing anything — copy their shape:
  - Creature NPC: `assets/overworld_objects/archer_goblin/metadata.yaml`
  - Talking/merchant NPC: `assets/overworld_objects/villager/metadata.yaml`
  - Weapon: `assets/overworld_objects/bronze_sword/metadata.yaml`
  - Consumable: `assets/overworld_objects/apple/metadata.yaml`
  - Scroll: `assets/overworld_objects/bless_scroll/metadata.yaml`
  - Base classes: `assets/object_bases/{npc,equipment,consumable,pickup}.yaml`
  - Quest dialog: `assets/dialogs/demo_villager.yarn`
  - Kill-quest script: `assets/quests/hunter.py`
  - Spell: `assets/spells/fireball.yaml`; Recipe: `assets/recipes/mushroom_brew.yaml`
- Extract `module-id` and the default `tier` from the `<!-- ... -->` comment.
  `MODULE_DIR` = `assets/modules/<module-id>`. Collect every entity: section,
  display name, `id` (the `(id: …)` suffix), prose, and parsed `hints`.

### 2. Resolve references and plan (do this before writing)

Collect the module's own entity ids (its `### … (id: x)` headings), then resolve
**every referenced id** (quest `giver`/`reward`/`objective` items, `drops`,
`shop` wares, recipe I/O, scroll `spell`) to an absolute id using the resolution
rules above. For each resolved reference, verify the target exists:

- in-module (`<module-id>/x`) → it's one of this module's entities, or you're
  about to create it. OK.
- core (`x`) → confirm `assets/overworld_objects/x/`, `assets/spells/x.yaml`,
  etc. exists.
- cross-module (`other_mod/x`) → confirm `assets/modules/other_mod/...` defines it.

If any reference resolves to nothing — a bare id that is neither in this module
nor in core — **stop and report the full unresolved list**; ask whether to stub
it or fix the module. Never silently invent a target.

Record the resolved absolute id for every reference; you'll write those into the
emitted files in steps 3–6. (No global-collision check is needed — the
`<module-id>/` prefix makes your entities unique by construction. The only
in-module rule is that your own local ids are unique within the module.)

### 3. Emit object YAML (NPCs and items)

For each new NPC/item, write `MODULE_DIR/overworld_objects/<local-id>/metadata.yaml`
— keep the **bare local id** as the directory name; the loader registers it as
`<module-id>/<local-id>` automatically. Use the prose for `name` and
`description`. **Always** set `render.debug_color` (an RGB tuple matching the
entity's vibe) and `debug_size`, so it is visible even if art generation is
skipped. Fill mechanical fields from `hints`, falling back to the tier tables in
the guide (§3–§4). Any id *inside* the YAML (a loot `type_id`, `shop` ware,
`spell_id`) must be the **resolved absolute id** from step 2 — bare for core
(`copper_coin`), `<module-id>/x` for your own, `other/x` for cross-module.

**Every NPC needs an `npc_behavior:` block** — it is the signal that marks the
object as an NPC, so the engine attaches the `Npc` marker and the entity is
talkable / targetable and spawns correctly via the editor, a map, or
`world.spawn`. `npc_behavior.hostile: true` makes it chase/attack on sight
(creatures); `false` keeps it stationary and peaceful (questgivers, townsfolk,
merchants). Give every NPC `stats` and `hp` too, so it has real vitals.

**Hostile creature** (mirror `archer_goblin`; `damage` is top-level, `damage_type`
lives in `attack_profile`):

```yaml
extends: npc
name: Cellar Rat
description: A fat, bold rat grown sleek on spoiled flour.
level: 1
loot:
  corpse_despawn_seconds: 60
  drops:
    - type_id: copper_coin
      quantity: 2
      probability: 1.0
stats: { strength: 6, agility: 12, constitution: 6, willpower: 4, charisma: 3, focus: 4 }
attack_profile:
  kind: melee          # ranged creatures also set: base_range_tiles, ammo_type (siblings)
  damage_type: pierce
damage: "1d3"          # dice only; engine adds the stat modifier
hp: "1d6+constitution"  # scale the constant to the tier table HP (guide §4)
npc_behavior:
  hostile: true              # chases + attacks on sight
  step_interval_seconds: 0.8
  detect_distance_tiles: 6
  disengage_distance_tiles: 10
render:
  debug_color: [120, 110, 100]
  debug_size: 0.6
  y_sort: true
```

**Talking NPC / quest-giver** (peaceful, stationary — still a *complete* NPC with
stats + an `npc_behavior:` block so it's talkable when spawned):

```yaml
extends: npc
name: Old Maple
description: A translucent, stooped badger in a flour-dusted apron.
dialog_node: OldMaple        # MUST equal the Yarn node title (step 4)
level: 2
stats: { strength: 6, agility: 8, constitution: 10, willpower: 12, charisma: 11, focus: 13 }
hp: "2d8+constitution"
npc_behavior:
  hostile: false             # stands its ground; talk-only
  step_interval_seconds: 1.5
  detect_distance_tiles: 5
  disengage_distance_tiles: 8
render:
  debug_color: [150, 180, 210]
  debug_size: 0.9
```

Merchants add a `shopkeeper:` block (see `villager`): `wares: [{ type_id,
price_copper, stock }]` where `stock` is an integer or `infinite`.

**Weapon** (mirror `bronze_sword`):

```yaml
extends: equipment
name: <Name>
weight: 2.0
description: <prose>
equipment_slot: weapon
attack_profile: { kind: melee, damage_type: cut }
render: { debug_color: [r, g, b], debug_size: 0.55 }
```

Other equipment: set `equipment_slot` (`armor`/`helmet`/`legs`/`shield`/`ring`/
`amulet`/`boots`/`backpack`) and `stats: { … }` for bonuses (armor/trinkets).

**Consumable** (mirror `apple`): `extends: consumable`, `weight`, `description`,
`use_effects` (`restore_health`/`restore_mana`, or `regen_multiplier` +
`regen_duration_seconds`), `use_texts`, optional `use_on_texts`.

**Scroll** (mirror `bless_scroll`): `extends: pickup`, `weight: 0.1`,
`description`, `spell_id: <existing or module spell id>`.

Plain quest item with no mechanics: `extends: pickup` + `name`/`description`/`render`.

### 4. Emit dialog (Yarn)

Write one file `MODULE_DIR/dialogs/<module-id>.yarn` containing **one node per
talking NPC** (any NPC with `gives_quest`, `role: townsfolk/merchant/questgiver`,
or `dialog: true`). Each node's `title:` must exactly equal that NPC's
`dialog_node`, and titles are global, so prefix them with the module to stay
unique (e.g. `HauntedMillOldMaple`). Mirror `demo_villager.yarn`.

For a **quest-giver**, generate the standard state-gated branch structure and
`<<declare>>` every variable up top — **except the shared `skill_check` system
variables** `$last_skill_check_success` / `$last_skill_check_total`, which are
declared once project-wide in `assets/dialogs/_system_vars.yarn`. Yarn variables
share one global namespace, so re-`<<declare>>`ing an existing variable is a hard
Y001 compile error that crashes the client. Declare only your **own** module
variables (prefix them with the module, e.g. `$<module-id>_<quest>_started`, so
they can't collide with another module's). The example below is a **fetch**
quest — pure Yarn, tracked with `<<set>>` variables, gated on `has_item(...)`:

```yarn
title: HauntedMillOldMaple
---
<<declare $last_grain_started = false>>
<<declare $last_grain_done = false>>

<<if $last_grain_done>>
    Old Maple: Bless you, traveler. The grain is milled and I can rest.
<<elseif $last_grain_started>>
    <<if has_item("haunted_mill/moonshade_grain", 1)>>
        Old Maple: You've brought it! Let me see...
        -> Hand over the grain.
            <<take_item "haunted_mill/moonshade_grain" 1>>
            <<give_item "potion" 1>>
            <<give_item "copper_coin" 20>>
            <<set $last_grain_done to true>>
            Old Maple: One last good milling. Thank you.
    <<else>>
        Old Maple: Have you found the moonshade grain in the cellar?
    <<endif>>
<<else>>
    Old Maple: The rats... and one last sack I never milled. Would you help?
    -> I'll help you, Maple.
        <<set $last_grain_started to true>>
        Old Maple: Bless you. Clear the cellar, then bring me the grain.
    -> Not now.
        Old Maple: I'll wait. I have nothing but time.
<<endif>>
===
```

Note the `give_item`/`take_item`/`has_item` ids are the **resolved absolute ids**
from step 2: `haunted_mill/moonshade_grain` (this module's item) but bare
`potion` / `copper_coin` (core). For a kill quest the same `<<start_quest>>` /
`<<complete_quest>>` argument must be the qualified `<module-id>/<quest-local-id>`.

Wiring rules (these are the easy things to get wrong):

- **`<<start_quest>>` / `<<complete_quest>>` require a registered `.py` quest
  module** (a quest id with no matching script in step 5 makes the engine
  `warn!` and no-op). So **only kill/event quests use them.**
- **fetch / talk** quests are pure Yarn (no `.py`): track progress with
  `<<set $<quest>_started to true>>` / `<<set $<quest>_done to true>>`, gate the
  turn-in branch on `<<if has_item("<item>", <n>)>>`, and consume with
  `<<take_item>>`. Do **not** call `start_quest`/`complete_quest`. (This mirrors
  the apples quest in `demo_villager.yarn`.)
- **kill** quests (step 5) call `<<start_quest "<module-id>/<local-id>">>` on
  accept and `<<complete_quest "<module-id>/<local-id>">>` on turn-in, and gate
  the turn-in branch on the quest variable the script sets (e.g.
  `$<module-id>_<local-id>_ready`).
- Hand out every `reward` item with `<<give_item "<resolved-id>" <n>>>` on
  turn-in (bare for core rewards, `<module-id>/x` for module items).
- A `persuade_bonus` adds a `[Persuade]` option gated on `skill_rank("Persuasion")
  > 0`, calling `<<skill_check "Persuasion" 15>>` and branching on
  `$last_skill_check_success` (mirror the persuasion branch in `demo_villager.yarn`).
  **Use** `$last_skill_check_success` / `$last_skill_check_total` — never
  `<<declare>>` them (they live in `_system_vars.yarn`; see step 4 intro).
- Use only verified commands: `give_item`, `take_item`, `give_recipe`,
  `start_quest`, `complete_quest`, `skill_check`, `stash_set`, plus standard
  `declare`/`set`/`if`/`elseif`/`endif`/`jump`. Read-only functions: `has_item`,
  `skill_rank`, `stash_has`, `stash_get_str/num/bool`.
- Non-quest townsfolk/merchant NPCs get a simple flavor node (a few lines, maybe
  a `<<jump>>` loop like `chatterbox.yarn`). Merchants need no special dialog
  command — the `shopkeeper:` YAML drives trading.

Then set `dialog_node: <NodeTitle>` in each talking NPC's `metadata.yaml`.

### 5. Emit quest logic — only for `kind: kill`

fetch/talk quests are **pure Yarn** (skip this step). For a kill quest, write
`MODULE_DIR/quests/<local-id>.py` (keep the bare local id as the filename — the
engine registers the quest under the qualified id `<module-id>/<local-id>`).
Inside, use that **qualified id** in `complete_quest`, and **module-prefixed**
Yarn variable names (matching the `<<declare>>`s in the dialog). The Yarn node
calls `<<start_quest "<module-id>/<local-id>">>` and
`<<complete_quest "<module-id>/<local-id>">>`. Mirror `hunter.py`:

```python
import mud_quest_api as q

subscribes_to = ["ObjectKilled"]
state = {"count": 0}

def on_start(state):
    state["count"] = 0
    q.set_var("<module-id>_<local-id>_started", True)
    q.set_var("<module-id>_<local-id>_ready", False)

def on_event(ev, state):
    if ev["kind"] != "ObjectKilled":
        return
    if ev["type_id"] != "<resolved-target-id>":   # e.g. "rat" (core) or "<module-id>/cellar_rat"
        return
    state["count"] += 1
    if state["count"] >= <N>:
        q.set_var("<module-id>_<local-id>_ready", True)

def on_command(name, args, state):
    if name == "complete":
        q.complete_quest("<module-id>/<local-id>")
```

The `<quest_id>_started` / `<quest_id>_ready` variable names must match the
`<<declare>>`d names in the Yarn node. Available `q.*` calls include `set_var`,
`get_var`, `complete_quest`, `fail_quest`, `give`, `take`, `spawn`, `teleport`,
`log` (full surface in `src/scripting_api/bindings.rs`). State values must be
JSON-shaped (str/int/float/bool/None/list/dict).

### 6. Emit spells and recipes (if the module has them)

- Spell → `MODULE_DIR/spells/<id>.yaml` (mirror `fireball.yaml`): `name`,
  `incantation`, `mana_cost`, `targeting` (`targeted`/`untargeted`/
  `targeted_tile`), `range_tiles`, optional `class_access`/`min_caster_level`,
  `effects` (`damage` + `damage_type`, `restore_health`, `aoe: { radius_tiles }`).
- Recipe → `MODULE_DIR/recipes/<id>.yaml` (mirror `mushroom_brew.yaml`): `name`,
  `description`, `inputs`, `outputs`, optional `station`, `auto_learn`,
  `xp_award`.

### 7. Generate sprites

For each **new** NPC and item (its `metadata.yaml` already written under
`MODULE_DIR` in step 3), invoke the `gen-sprite` skill (via the Skill tool) with
the entity id and a one-line appearance taken from its `appearance:` hint or
prose — e.g. `gen-sprite cellar_rat large grey rat with a flour-white muzzle`.
gen-sprite locates the object's metadata wherever it lives (global or under
`assets/modules/*/`) and writes the sheet beside it, so the module stays
self-contained. The `debug_color` set in step 3 guarantees a visible fallback, so
if any sprite is skipped or fails, the entity still works.

### 8. Finalize, verify, and report

- **Provenance**: copy the source module Markdown to `MODULE_DIR/module.md` so the
  compiled pack records what it came from.
- **Cross-references resolve**: re-check that every `<<give_item>>`/`<<take_item>>`
  type_id, every `loot`/`shop`/recipe id, every scroll `spell_id`, and every
  `dialog_node` ↔ Yarn `title` pair matches. Quest id == `.py` stem ==
  `start_quest`/`complete_quest` argument.
- **Compile gate**: run `cargo check` (must stay clean). Note: `cargo check` does
  **not** parse the YAML/Yarn assets — it only confirms the project still builds.
- **Load gate (the real validator)**: run `cargo run --bin mud2` (or `--bin
  server`) and confirm the logs show **no warnings/errors** loading the new ids
  (the Rust loader is the authoritative schema). Fix any reported parse errors.
- **End-to-end (optional but recommended)**: test-spawn a new NPC via the map
  editor or the admin REPL (`world.spawn(...)` / `q.spawn`), then walk the loop:
  talk → start quest → satisfy objective → turn in → reward → completion branch.
  (Per design, the build makes entities *available*; placing them in a space is
  done manually.)
- **Report**: list every file created under `MODULE_DIR`, every existing id
  reused, every id-collision or missing reference flagged, and remind the user
  that the module loads automatically on next launch (and uninstalls by deleting
  `MODULE_DIR`). If `MODULE_DIR` already existed from a prior build, say which
  files you replaced rather than overwriting silently.
