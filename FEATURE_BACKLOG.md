# Nice-to-Have Feature Catalog

## Context

We've reached a point where the core loop works locally and over TCP: movement, combat, spells, containers, equipment, NPC AI, multi-space portals, and JSON persistence all function. Before planning the next batch of features, we want a single scannable doc that lists the obvious gaps — so whatever we pick next is chosen against the full menu, not in isolation. This is a living reference, not a commitment to build any of it; items near the top are higher-impact / higher-cost, items near the bottom are smaller polish wins.

`ISSUES.md` tracks specific bugs and near-term todos. This doc is complementary: it focuses on larger *systems* we haven't built yet, and which of them compose well together.

---

## 2. Other large features (each is a multi-week effort)

Grouped by system, not ranked.

### Progression
- **XP + levels** — no experience points anywhere today. Hook into combat death events; derived stats scale with level.
- **Skill levels** — per-skill advancement (sword, club, axe, distance, magic level, shielding, fishing). Tibia's identity is largely this; without it combat has no long-tail.
- **Vocations / classes** — knight / mage / ranger / paladin, gating spells and skill growth curves.
- **Death penalty** — currently "defeated" is a chat line with no cost. At minimum: respawn on a bed/temple tile, drop a configurable fraction of XP / skills.
- **Soul / stamina** — regen caps that shape play-session length.

### NPC depth
- **Dialogue system** — no `DialogueComponent`, no conversation UI, no authored NPC text. This unlocks vendors, quest-givers, flavor NPCs.
- **Vendors / trading** — buy/sell UI, per-NPC stock, currency item (gold). Inventory infrastructure (`InventoryStack`) can support it.
- **Quest log** — quest state persisted per player; journal panel (fits well as a `DockedPanelKind::QuestLog` — the docked window model in `PLAN.md` §6.9.1 already anticipates this).
- **Proper spawner / respawn** — NPCs currently despawn on death with no respawn; authored spawns don't replenish. Need spawn pools with interval and cap per area.
- **NPC ranged / caster AI** — only melee roam-and-chase exists (`src/npc/`).

### Combat depth
- **Armor / shield reduction** — equipment slots exist but damage is `d6 + str/5` with no defense (`src/combat/systems.rs:212`). Add flat/percent armor, shield block chance.
- **Ranged attacks** — no bow/wand path; no projectile entity.
- **Status effects** — poison, burn, bleed, haste, slow, paralyze. Needs a tick/duration component and UI badges.
- **Elemental damage types** — physical/fire/ice/earth/death/holy with resistances.
- **Critical hits, dodge, hit chance** — flat d6 currently.

### Magic expansion
- **Cooldowns** — currently only mana-gated; spam-castable.
- **Mana regen** — only potions restore mana.
- **Spellbook UI + spell learning** — today only two spells exist as scrolls (`assets/spells/`); no way to "know" a spell.
- **AoE + rune system** — Tibia-like runes (targetable consumables) fit naturally on top of the existing scroll model.
- **Buffs/debuffs from spells** — needs the status-effect system above.

### Multiplayer / backend
- **Authentication + character selection** — title screen has a connect flow but no login, no character roster, no account persistence (`src/network/`, `src/app/title.rs`).
- **Reconnection** — dropping == despawn. Need session tokens and a grace window.
- **Interest management (AoI)** — currently every client hears every event regardless of distance (`compute_events_for_peer` broadcasts). Breaks past a small player count.
- **Client prediction + interpolation** — remote player movement is already smoothed (`VisualOffset` / `JustMoved`), but the local player has no prediction; high latency = visible lag.
- **TLS / rate limiting / anti-cheat** — plaintext JSON TCP, no throttling on commands.

### Content pipeline
- **YAML brushes / ranges** — authoring tile lists is verbose (ISSUES.md already flags). Add rectangles, floods, templates.
- **Hot reload** — assets require restart. Bevy asset change events could drive live reload for object metadata and spells.
- **Data-driven NPC behaviors** — `RoamingBehavior` / `HostileBehavior` are code-defined; map YAML can only pick from the hardcoded set.
- **Server-side scripting / triggers** — RustPython is client-side dev only. A "tile enter → do X" trigger system would unblock quests and events without new code.

---

## 3. Medium features (days, not weeks)

### World interactables
- **Doors + keys + locks** — no door type exists; big gameplay unlock for dungeons.
- **Levers / pressure plates** — simple mechanical puzzle primitives.
- **Signs / readable props** — a `ReadableText` component + popup modal.
- **Ladders / stairs / ropes / holes** — listed under stacking layers but valuable even without full z-levels if we fake it with portals.

### Chat & social
- **Chat input** — chat log exists (`ChatLogText`) but there is no /say, /shout, /whisper, /emote command path.
- **Channels** — local vs global vs private.
- **Party / group** — shared target highlighting, XP split.

### UI / HUD
- **Damage numbers / floating combat text** — no toast system; currently only chat lines.
- **Hotbar / quick slots** — no keybind-driven use slots for potions or spells.
- **Minimap** — absent. Floor-aware once z-levels land.
- **Settings menu + keybind customization** — all input is hardcoded.
- **Zoom + camera control** — fixed zoom, player-follow only.
- **Inspect/examine panel** — could migrate onto the docked-panel system (already designed in `PLAN.md` §6.9.1).

### Items
- **Durability / wear** — no break mechanic.
- **Rarity tiers + enchantments** — flat equipment today.
- **Food / hunger** — apples heal instantly; no hunger meter.
- **Item-on-item crafting / combining** — no recipe system.
- **Ground-item decay timers** — corpses persist forever after save.

### Ops / infra
- **Autosave on interval** — only shutdown saves; crash loses everything.
- **Crash recovery / atomic save** — single JSON file; partial writes corrupt.
- **Per-character save files** — monolithic world dump makes account growth painful.
- **Save versioning + migration** — save format is `v3` but no migration path.
- **Structured logging / game.log** — bare `bevy::log` only.
- **Admin commands** — /teleport, /godmode, /noclip, /spawn (only Python `spawn_object` exists today).
- **Server console** — headless mode has no REPL input.
- **Debug overlay** — FPS, entity count, tile/space inspector. Toggleable HUD.

### Audio
- **Nothing exists** — no Bevy audio plugin loaded. Object metadata already has `sound_paths` fields that are unused, so wiring is partly anticipated.

---

## 4. Small polish

- Toast notifications for level-up, loot pickup, quest updates.
- Cursor-hover tooltips for ground objects.
- Cursor set is only 3 entries (`assets/cursors/`); attack/talk/push variants would help.
- Gamepad support.
- Colorblind-friendly damage colors, UI font scaling.
- CI config (`.github/workflows/`) — none exists.
- Unit tests — only one integration test (`tests/multiplayer_transport.rs`, `#[ignore]`d).

---

## 5. Suggested batching

Some features share infrastructure and are cheaper built together. Rough clusters in case you want to scope a "batch":

- **Stacked floors batch**: TilePosition-z, floor-aware rendering / collision, stairs/ladders/ropes/holes, editor floor selector, save format bump, floor indicator HUD.
- **Progression batch**: XP + levels + skill levels + death penalty (needs a respawn point and a way to drop items/skills) — these share the same combat death hook.
- **Living NPC batch**: Dialogue system + vendors + quest log + spawner/respawn — dialogue is a prerequisite for the other three.
- **Combat depth batch**: Armor/shield + ranged attacks + status effects + elemental damage + critical/dodge — one pass through `src/combat/systems.rs` covers all of them.
- **Production-readiness batch**: Autosave + atomic writes + save versioning + structured logging + admin commands + debug overlay — roughly the `PLAN.md` Phase 8 (Production Hardening) set.
- **Multiplayer scaling batch**: AoI + auth/character select + reconnection + rate limiting. Has to happen before the server sees non-trivial load.

---

## 6. Out of scope for this doc

- Bug fixes and in-flight migrations (tracked in `ISSUES.md`).
- Implementation plans for any individual feature above — those come once one is picked.
- Content volume (more maps, more monsters, more items). Content grows continuously and isn't a "feature".

---

## Living-document rules

Update when:
- A feature here is completed (move it to `ISSUES.md`'s Completed list and delete from here).
- A new gap becomes obvious while working on something else.
- A planned feature's scope changes materially (e.g. stacking layers turns out to require a different data model).
