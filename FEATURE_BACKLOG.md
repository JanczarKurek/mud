# Nice-to-Have Feature Catalog

## Context

The core loop works locally and over TLS-capable TCP: movement, combat,
magic (with class-gated spells and level-scaled caster mana), containers,
equipment, NPC AI (melee + kiting ranged) with spawn pools, multi-space
portals, account-level persistence, character creation with a class
picker, dialog (yarnspinner), a Python-scripted quest engine with a
docked quest-log panel, vendors / trading, basic crafting with recipes,
chat input, currency + pouches + carry weight, XP/levels with classes
(Phase A/B), partial-drop death penalty (Phase D), HP/mana regen with
food buffs, floor-type tiling, an in-app map editor, and an F3–F12
diagnostics overlay all function.

This doc lists *systems-sized* gaps so whatever we pick next is chosen against
the full menu. It's a reference, not a commitment. Items near the top are
higher-impact / higher-cost.

`ISSUES.md` tracks bugs and near-term todos. `PLAN.md` carries the broad
phasing.

---

## 1. Big features (multi-week each)

Grouped by system, not ranked.

### Progression

Designed in **`docs/progression.md`** (D&D 3.5e-flavored: classes
Fighter/Wizard/Cleric/Vagabond, XP+levels, 10-skill sheet with skill
points, level-scaled mana, partial-drop death penalty). Phasing A–E
lives in that doc §9 and is tracked from `PLAN.md` §4.2. All five phases
are shipped, including **Phase C** — `SkillSheet`, point spending, and
the `skill_check` helper plus its first in-game consumers (locks,
Persuasion-driven vendor pricing, Yarn `<<skill_check>>`). Skills
deferred to future batches (Stealth/Perception/Survival/Spellcraft/etc.)
are tracked in `docs/skills_locks_social_plan.md`.

Additional progression-adjacent items not in scope of that doc:
- **Soul / stamina** — regen caps that shape session length. Independent
  of the level/skill loop; lands when we want to gate grind sessions.

### NPC depth
- **NPC caster AI** — only melee roam-and-chase and ranged kiter exist. No mob can cast spells.

### Combat depth
- **Armor / shield reduction** — equipment slots exist but damage is `d6 + str/5` with no defense (`src/combat/systems.rs`). Add flat/percent armor, shield block chance.
- **Status effects** — poison, burn, bleed, haste, slow, paralyze. Needs a tick/duration component and UI badges.
- **Elemental damage types** — physical/fire/ice/earth/death/holy with resistances.
- **Critical hits, dodge, hit chance** — flat d6 currently.
- **Cooldowns** — spells are mana-gated only; spam-castable.

### Magic expansion
- **Spellbook UI + spell learning** — `assets/spells/` now has 13 spells with `class_access` / `min_caster_level` gating, but spells are still scroll-only; there is no docked spellbook and no way for a character to "know" a spell.
- **AoE + rune system** — Tibia-style runes (targetable consumables) fit on top of the existing scroll model.
- **Buffs / debuffs from spells** — depends on the status-effect system above.

### Multiplayer / backend
- **Interest management (AoI)** — `compute_events_for_peer` broadcasts everything. Bandwidth blows up past a small player count. **The single biggest scaling blocker.**
- **Reconnection** — disconnect == despawn. Need session tokens and a grace window.
- **Client prediction for the local player** — remote movement is smoothed via `VisualOffset` / `JustMoved`; the local player has no prediction so high latency = visible lag.
- **Multiple characters per account** — schema migration + protocol variants + character-select screen (also tracked in `ISSUES.md`).
- **Rate limiting + plaintext-warning + TOFU pinning** — see `ISSUES.md`.

### Content pipeline
- **YAML brushes / ranges** — authoring tile lists is verbose. Add rectangles, floods, templates.
- **Hot reload** — assets require restart. Bevy asset change events could drive live reload for object metadata and spells.
- **Data-driven NPC behaviors** — `RoamingBehavior` / `HostileBehavior` are code-defined; map YAML can only pick from the hardcoded set. Add behavior templates / parameters in YAML.
- **Server-side scripting / triggers** — RustPython is currently used for quests and the dev console. A "tile enter → run script" trigger system would unblock events without new code.
- **Map editor floor selector** — paused from the stacked-floors PoC; see `docs/stacked_floors_plan.md`.

---

## 2. Medium features (days, not weeks)

### World interactables
- **Keys + locks** — `wooden_door` already has open/closed states and a lever can drive it via `side_effects: set_target_state`; the missing piece is a key/lock gate before the open transition fires.
- **Pressure plates** — levers exist (with `wires_to` + `side_effects`); pressure plates would reuse the same wiring schema with an on-enter trigger.
- **Signs / readable props** — `sign_post` asset exists but no `ReadableText` component / popup modal.
- **Ladders / ropes** — listed in the stacked-floors Phase 3 backlog (`sinkhole` is already a working "hole" instance).

### Chat & social
- **Channels** — local vs global vs private.
- **Party / group** — shared target highlighting, XP split.

### UI / HUD
- **Damage numbers / floating combat text** — currently only chat lines.
- **Hotbar / quick slots** — no keybind-driven use slots for potions or spells.
- **Settings menu + keybind customization** — input is hardcoded.
- **Zoom + camera control** — fixed zoom, player-follow only.
- **Inspect / examine panel** — fits `DockedPanelKind` model. Inventory item tooltips on hover already exist; ground-object hover tooltips are still missing.
- **Floor indicator HUD** — small but missing (paused from stacked-floors Phase 2).

### Items
- **Durability / wear** — no break mechanic.
- **Rarity tiers + enchantments** — flat equipment today.
- **Hunger meter** — food items grant a temporary regen buff (`RegenBuffs` in `src/player/regen.rs`), but there is no hunger gauge that decays over time and gates eating.
- **Ground-item decay timers** — corpses persist forever after save.
- **Pouch panel auto-retarget** — moving a pouch from backpack slot 2 → 7
  while the pouch panel is open silently closes the panel rather than
  retargeting it to slot 7. Low priority — re-opening is one right-click.

### Ops / infra
- **Atomic save writes** — single JSON file; partial writes can corrupt.
- **Save format migration** — format_version has crept past 7 with implicit fallbacks plus the standalone `scripts/migrate_save_v8_to_v9.py`; no explicit migration framework if the schema breaks.
- **Per-character separate save files** — `accounts.db` already separates players from world dump, but each player's character blob is monolithic.
- **Structured logging / `game.log`** — `bevy::log` only.
- **Admin commands** — `/teleport`, `/godmode`, `/noclip`, `/spawn` from in-game chat (the headless-server admin REPL over UNIX socket already gives full Python world access).

### Audio
- **Nothing exists.** No Bevy audio plugin loaded. Object metadata already has unused `sound_paths` fields.

---

## 3. Small polish

- Toast notifications for loot pickup and quest updates (level-up toast is shipped — see `LevelUpToast`).
- Cursor-hover tooltips for ground objects (inventory item tooltips already exist via `sync_item_tooltip`).
- More cursors (attack/talk/push variants); `assets/cursors/` is sparse.
- Gamepad support.
- Colorblind-friendly damage colors, UI font scaling.
- CI config (`.github/workflows/`) — none exists.
- Real test coverage — unit tests in modules plus `tests/multiplayer_transport.rs` and `tests/admin_repl.rs`. Networked-flow coverage is still thin.

---

## 4. Suggested batching

Some features share infrastructure and are cheaper built together. Rough clusters
in case you want to scope a "batch":

- **Stacked floors finish batch**: Editor floor selector + `FloorIndicatorLabel` HUD + ladder/rope transitions. Wraps up the paused Phase 2/3 of `docs/stacked_floors_plan.md` (yaml schema docs and `sinkhole` already shipped).
- **Combat depth batch**: Armor/shield + status effects + elemental damage + critical/dodge — one pass through `src/combat/systems.rs` covers all of them. Picks up Phase C (skills) too if `SkillSheet` lands at the same time, since both touch the to-hit/damage path.
- **Production-readiness batch**: Atomic save writes + save migration framework + structured logging + in-game admin commands (≈ `PLAN.md` Phase 8). Diagnostics overlay is already shipped.
- **Multiplayer scaling batch**: AoI + reconnection + multi-character per account + rate limiting + TOFU pinning. Has to happen before non-trivial player count.

---

## 5. Out of scope for this doc

- Bug fixes and in-flight migrations (tracked in `ISSUES.md`).
- Implementation plans for any individual feature above — those come once one is picked.
- Content volume (more maps, more monsters, more items). Content grows continuously and isn't a "feature".

---

## Living-document rules

Update when:
- A feature here is completed (move it to `ISSUES.md`'s Completed list and delete from here).
- A new gap becomes obvious while working on something else.
- A planned feature's scope changes materially.
