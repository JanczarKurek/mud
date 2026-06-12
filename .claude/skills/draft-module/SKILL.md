---
name: draft-module
description: Draft a readable Markdown "world module" (an RPG-scenario design doc — locations, NPCs, items, quests) for this game from a seed idea. Use when the user wants to sketch new world content as prose before compiling it into game data. The companion skill build-module compiles the module.
argument-hint: "<seed idea, e.g. 'a haunted riverside mill with a miller's ghost'>"
allowed-tools: Read Write Glob
---

Draft a world module for the seed idea: **$ARGUMENTS**

A *module* is a readable, prose-first design document — a tabletop-RPG-style
scenario — that a designer reviews and then compiles into real game content with
the `build-module` skill. Your job here is **only** to write the Markdown; do not
create any `assets/` files.

## Your task

### 1. Read the contract

- Read `docs/modules/AUTHORING_GUIDE.md` **in full**. It is the single source of
  truth for the world, tone, mechanics, tier framework, content menus, and the
  exact module format (§5) — including the `id` convention and the `hints`
  reference. Everything you write must conform to it.
- Skim `docs/content_bible.md` and `docs/progression.md` only if the seed needs
  detail the guide doesn't cover (specific bestiary numbers, class flavor).

### 2. Inventory existing content (so you reuse, not duplicate)

- `ls assets/overworld_objects/` — existing NPCs, items, scrolls, props you can
  reference directly by id as foes, rewards, or ingredients.
- `ls assets/spells/` and `ls assets/recipes/` — existing spells and recipes.
- Prefer an existing id over inventing a new entity. The guide §4 lists the
  common ones; the directory listing is the authoritative set.

### 3. Draft the module

Write the module to `modules/<slug>.md`, where `<slug>` is a short snake_case
name derived from the seed (e.g. `haunted_mill`). Use that same `<slug>` as the
`module-id` in the header comment — `build-module` compiles the module into a
folder named after it (`assets/modules/<slug>/`), so it must be unique and
stable. Follow the format in `AUTHORING_GUIDE.md` §5 exactly:

- `# Module: <Title>` then the `<!-- module-id: <slug> | tier: T? -->` comment.
- `## Overview` — a few paragraphs of pitch/hook/mood and how it connects to the
  wider world.
- `## Locations` — prose only (no geometry); each place an `### Name (id: …)`.
- `## NPCs`, `## Items`, `## Quests` — each entity an `### Name (id: …)` with
  rich prose and an optional ```hints``` block. Add `## Spells` / `## Recipes`
  only if the seed calls for them.

Hold to these while drafting:
- **Reuse** existing ids wherever they fit; invent new entities only when nothing
  fits, and give every new NPC/item an `appearance:` hint line for later sprite art.
- **Tag a tier** on the module and keep foes/rewards inside that band (guide §3–§4).
- **Cross-references must resolve**: every `giver`, `reward`, `drops`, and
  `objective` id must be either an existing asset id or an entity you define in
  this same module. Never reference something undefined.
- Keep quests to **fetch / kill / talk** (the kinds that compile cleanly).
- Stay in tone: cosy, pastoral, wry, a little eerie — never grimdark.

### 4. Report

Print the path you wrote, a one-line summary, and a short list of the entities
created (ids) plus which existing ids were reused. Remind the user to read and
edit the module, then run `build-module modules/<slug>.md` to generate the game
content.

Do **not** run `build-module` yourself or touch `assets/` — the module is a
human review checkpoint.
