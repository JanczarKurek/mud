# Nice-to-Have Feature Catalog

## Context

The core loop works locally and over TLS-capable TCP: movement, combat, magic,
containers, equipment, NPC AI (melee + kiting ranged), multi-space portals,
account-level persistence, dialog (yarnspinner), and a Python-scripted quest
engine all function. Floor-type tiling has just landed (corner-aware grass/dirt
transitions, tile variants).

This doc lists *systems-sized* gaps so whatever we pick next is chosen against
the full menu. It's a reference, not a commitment. Items near the top are
higher-impact / higher-cost.

`ISSUES.md` tracks bugs and near-term todos. `PLAN.md` carries the broad
phasing.

---

## 1. Big features (multi-week each)

Grouped by system, not ranked.

### Progression
- **XP + levels** — combat death events exist; need per-player XP, level curve, derived-stat scaling.
- **Skill levels** — per-skill advancement (sword, club, axe, distance, magic level, shielding, fishing). Tibia's identity. Without it combat has no long-tail.
- **Vocations / classes** — knight / mage / ranger / paladin gating spells and skill curves.
- **Death penalty** — currently "defeated" is just a chat line. Need respawn on a bed/temple tile and configurable XP/skill loss.
- **Soul / stamina** — regen caps that shape session length.

### NPC depth
- **Vendors / trading** — buy/sell UI, per-NPC stock, currency item (gold). Dialogue is in place; the missing piece is a vendor panel and currency wiring.
- **Quest log UI** — quest *engine* exists (`src/quest/`); there is no docked panel showing accepted/completed quests. Fits well as `DockedPanelKind::QuestLog`.
- **Spawner / respawn** — NPCs despawn on death with no respawn; authored spawns don't replenish. Need spawn pools with interval and cap per area.
- **NPC caster AI** — only melee roam-and-chase and ranged kiter exist. No mob can cast spells.

### Combat depth
- **Armor / shield reduction** — equipment slots exist but damage is `d6 + str/5` with no defense (`src/combat/systems.rs`). Add flat/percent armor, shield block chance.
- **Status effects** — poison, burn, bleed, haste, slow, paralyze. Needs a tick/duration component and UI badges.
- **Elemental damage types** — physical/fire/ice/earth/death/holy with resistances.
- **Critical hits, dodge, hit chance** — flat d6 currently.
- **Cooldowns** — spells are mana-gated only; spam-castable.

### Magic expansion
- **Mana regen** — only potions restore mana.
- **Spellbook UI + spell learning** — only two spells exist as scrolls (`assets/spells/`); no way to "know" a spell.
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
- **Doors + keys + locks** — no door type exists; big dungeon unlock.
- **Levers / pressure plates** — simple mechanical puzzle primitives.
- **Signs / readable props** — `sign_post` asset exists but no `ReadableText` component / popup modal.
- **Ladders / ropes / holes** — listed in the stacked-floors Phase 3 backlog.

### Chat & social
- **Chat input** — chat log exists (`ChatLogText`) but there is no `/say`, `/shout`, `/whisper`, `/emote` command path.
- **Channels** — local vs global vs private.
- **Party / group** — shared target highlighting, XP split.

### UI / HUD
- **Damage numbers / floating combat text** — currently only chat lines.
- **Hotbar / quick slots** — no keybind-driven use slots for potions or spells.
- **Settings menu + keybind customization** — input is hardcoded.
- **Zoom + camera control** — fixed zoom, player-follow only.
- **Inspect / examine panel** — fits `DockedPanelKind` model.
- **Floor indicator HUD** — small but missing (paused from stacked-floors Phase 2).

### Items
- **Durability / wear** — no break mechanic.
- **Rarity tiers + enchantments** — flat equipment today.
- **Food / hunger** — apples heal instantly; no hunger meter.
- **Item-on-item crafting / combining** — no recipe system.
- **Ground-item decay timers** — corpses persist forever after save.

### Ops / infra
- **Atomic save writes** — single JSON file; partial writes can corrupt.
- **Save format migration** — format_version is 7 with implicit fallbacks; no explicit migration framework if the schema breaks.
- **Per-character separate save files** — `accounts.db` already separates players from world dump, but each player's character blob is monolithic.
- **Structured logging / `game.log`** — `bevy::log` only.
- **Admin commands** — `/teleport`, `/godmode`, `/noclip`, `/spawn` (only Python `spawn_object` exists today).
- **Server console** — headless mode has no REPL input.
- **Debug overlay** — FPS, entity count, tile/space inspector, toggleable HUD.

### Audio
- **Nothing exists.** No Bevy audio plugin loaded. Object metadata already has unused `sound_paths` fields.

---

## 3. Small polish

- Toast notifications for level-up, loot pickup, quest updates.
- Cursor-hover tooltips for ground objects.
- More cursors (attack/talk/push variants); `assets/cursors/` is sparse.
- Gamepad support.
- Colorblind-friendly damage colors, UI font scaling.
- CI config (`.github/workflows/`) — none exists.
- Real test coverage — currently mostly unit tests in modules + one `#[ignore]`d integration test (`tests/multiplayer_transport.rs`).

---

## 4. Suggested batching

Some features share infrastructure and are cheaper built together. Rough clusters
in case you want to scope a "batch":

- **Living-world batch** *(builds on shipped dialog + quest engine)*: Vendor UI + currency item + Quest log panel + NPC spawners — these are the four things that turn the existing dialog/quest tech into actual content. Lowest activation energy of any batch.
- **Stacked floors finish batch**: Editor floor selector + `FloorIndicatorLabel` HUD + ladder/rope/hole transitions + `yaml_formats.md` z documentation. Wraps up the paused Phase 2/3 of `docs/stacked_floors_plan.md`.
- **Progression batch**: XP + levels + skill levels + death penalty (needs respawn point + skill drop). All share the combat death hook.
- **Combat depth batch**: Armor/shield + status effects + elemental damage + critical/dodge — one pass through `src/combat/systems.rs` covers all of them.
- **Production-readiness batch**: Atomic save writes + save migration + structured logging + admin commands + debug overlay (≈ `PLAN.md` Phase 8).
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
