# Issues and Ideas

Short bug/risk/idea log. Bigger system work belongs in `FEATURE_BACKLOG.md`;
the project's broad direction lives in `PLAN.md`.

## Active

### Auth & operations
- Account admin tool (`mud2-admin` binary) for password reset, account ban, account deletion.
- Multiple characters per account — schema migration (reintroduce a `characters` table) plus `ListCharacters` / `SelectCharacter` / `CreateCharacter` protocol variants and a character-select screen.
- Rate limiting on auth attempts (currently unbounded — an open server is susceptible to brute force).
- Username / password validation policy beyond the v1 minimums — `validate_username` / `validate_password` (`src/accounts/db.rs`) cover length and allowed chars; still open: leaked-password check, stronger entropy rules.
- When `--tls` is off and the server binds to a non-loopback address, emit a startup warning so operators don't accidentally run cleartext over the public internet.

### TLS hardening
- `--ca-cert PATH` client TLS trust anchor so self-signed dev certs can be verified without `--insecure`.
- TOFU (trust-on-first-use) fingerprint pinning for client TLS; store fingerprints in `~/.local/share/mud2/known_hosts`.

### Architecture / cleanup
- Finish migrating remaining presentation systems to consume replicated/view state instead of directly reading authoritative ECS/resources.
- Decide when to delete the now-obsolete direct-mutation helpers still left in `ui::systems`.
- Add dedicated ECS query helpers/system params for common same-space access patterns so AI and interaction systems stop hand-filtering residents.

### Dialog & lighting (latent, found 2026-07-30)
- `has_dialog` replication never validates that the Yarn node actually exists; a typo'd `dialog_node` (or a node in a yarn file excluded at startup for parse errors) yields a visible Talk entry whose click now no-ops with an error log — `try_start_node` replaced the panicking `start_node` on 2026-08-04. A load-time check of `dialog_node` values against compiled node titles would catch it at the source and hide the button.
  Context: before 2026-08-04, one unparsable `.yarn` file wedged the *entire* Yarn project — the asset never finished loading, `YarnProject` was never inserted, and every dialog in the game went silently mute (this is how the Hollow Bell module's two missing `<<endif>>`s killed all Talk buttons). `dialog/plugin.rs` now pre-parses each file and excludes broken ones individually with an ERROR log, and `cargo test --lib` parses every repo yarn file; the per-file enumeration also retired the old CWD-vs-asset-root mismatch on the `assets/modules` folder source (discovery is still CWD-rooted, consistent with `crate::assets::module_dirs_with_names`).
- `darkness.rs` builds its per-tile indoor bitmask from *object* occluders only, while `IndoorTileMap` (`floors.rs`) also counts painted upper-floor tiles — tiles under a painted storey get both the indoor sprite tint and the outdoor overlay (double darkening).
- `gen_schemas` no longer compiles (`cargo check --features gen-schemas`): several types referenced from schema'd definitions (`AttributeSet`, `Class`, `Direction`, `LootTableDef`, `BabTrack`, `Skill`, `WallCorner`) lack `JsonSchema` derives, so `assets/schemas/*.json` is stale (predates the balance/creature fields and `lighting.darkness_color`).

### Quests & journal (noted 2026-08-04, during the quest-log fix)
- **HeadlessServer has no dialog runtime** (`src/app/plugin.rs` skips `DialogServerPlugin` because YarnSpinner needs `AssetPlugin`), so `<<start_quest>>` — the only quest-start path — is unreachable in dedicated-server mode: quests cannot begin over TCP play. The declarative journal evaluator itself runs headless fine (it reads restored yarn vars); it's dialog execution that's missing.
- bevy_yarnspinner drops unregistered/typo'd `<<commands>>` with no log at all. Mitigated for quest ids by the `yarn_quest_ids_have_registered_scripts` test; a general "unknown command" observer/lint would catch the rest (pairs with the `dialog_node`-validation idea above).
- `assets/modules/hollow_bell/module.md` reward drift vs the shipped yarn: `down_the_shaft` promises `torch x10` (yarn gives `potion x2` + `silver_coin x30`), and `the_deeplistener` lists `deeplisteners_ear x1` as a reward (it's kill-loot only — deliberate per the comment in the quest script; the design doc is stale).

### Combat & progression balance
The **balance retune** shipped on top of the earlier BAB batch (see the rewritten
`docs/balance/report.md` §1 for the full scaling scheme): modifier-based weapon/spell damage
(`str_mod`/`agi_mod`/`foc_mod` expression terms + `level/2` growth on real weapons), a
level-scaled dodge DC (`10 + 3L/4 + AGI_mod + items`), critical hits (`crit_range`, double
damage roll, dagger 19–20), Backstab (sneaking+undetected opener; Vagabond `1+L/4` d6),
Fighter Weapon Focus, full-value armor + retuned values, scalable heals/DoTs
(`ScalableEffectSpec`), mana-regen retune (`2 + WIL + FOC/2`/min), linear XP (`75·level`,
~13 kills/level), a creature re-budget (dmg ≈ `2+1.3·L`, HP ≈ `20+11·L`; rat coherence fixed),
and a gear tier to ~L12 (iron/steel swords, dagger, longbow, chain set, plate, tower shield;
Ogre Brute L10 + Dire Wight L12 elites carry the loot). Remaining follow-ups:
- **Two-handed `STR×1.5`** damage and **finesse** weapons (AGI-keyed melee to-hit).
- **Damage-type resistances** — types are still cosmetic; monster-Lore reveals want them.
- **NPC AoE now fans out**: a tile-targeted NPC cast damages every *player* in
  `aoe.radius_tiles`, not only the current target (`build_npc_cast_outcome` emits an
  `NpcAoeSplash`; `execute_npc_spell_cast` resolves it). Monsters deliberately do not
  friendly-fire their own adds. Open: player-side companions are also spared, which is a
  simplification rather than a considered rule.
- ~~**Boss variants** (2–3× budget + mechanics)~~ — first three shipped in the
  `hollow_bell` module (Cinderjack L8, Knell L11, The Deeplistener L14). Budget rule
  established: **HP ×2–3, damage only ×1.2–1.4** — tripling damage one-shots a
  level-appropriate character, so the budget goes into HP and mechanics. Mechanics use
  `spellcasting:` `!self_hp_below_fraction` gates as phase triggers, plus the two engine
  changes below. The L20 Ancient Dragon in the sim is still a standard-budget stat block.
- **Trap-build UX**: tools (pickaxe/herb knife) deliberately carry no level growth; surface an
  in-game hint that they aren't weapons.
- AOL-style equipment **drop protection** hook on death (noted in `progression.md` §8 rule 3).
- Old world snapshots keep a stale persisted NPC `AttackProfile` until the NPC respawns
  (`persistence` prefers the dump); damage/crit resolve from current defs at snapshot time.
- ~~New-creature placement is provisional~~ — resolved by the Emberbrook overworld redesign:
  the Ogre Brute dens in NW Thornwood and the Dire Wight guards the founder's vault in the
  underworld (each with its own spawn group and lair).
- Utility skills **Lore / Spellcraft / Heal** still have no mechanic and **Survival** is a
  binary gather gate (`docs/utility_systems.md` Slices 4–5) — deferred from the combat-focused
  pass by design.

### Multi-floor (z-level) follow-up
The PoC in `docs/stacked_floors_plan.md` Phase 0/1 is shipped (z in `TilePosition`,
roof hiding, stairs, sinkhole as a "hole" transition). Schema docs for the
floor fields are now in `docs/yaml_formats.md`. Subsequent commits pivoted to
*floor-type tiling* (grass/dirt/stone transitions, tile variants) — so the
rest of Phase 2/3 of stacked floors is paused. See that doc for the open
items if/when we resume:
- Editor floor selector (PgUp/PgDn) + per-floor dimming.
- `FloorIndicatorLabel` HUD text.
- Ladder / rope transition object kinds (sinkhole already exists).

**Fixed:** floors are now solid to the *player* movement path too. Previously
only NPCs respected them (`spatial::apply_floor_layer` inserts the slab as a
pseudo-blocker); the player's `resolve_step_with_climb` and `resolve_landing_at`
used the raw column top, so stepping into any obstacle standing on an upper
floor cascaded past that floor to `z = 0`, a SHIFT-climb from inside a roofed
room landed on the roof, and a jump across a roofed room landed on the roof.
Both now go through `Column::surface_from` / `Column::slab_between`.
`settle_pending_stacks` likewise restarts its compaction at each painted floor
instead of running once from `z = 0`. Remaining sharp edge: an occluding-but-
unwalkable floor tileset would read as impassable to players while NPCs still
fall through it — no such tileset exists, and `walkable_surface` defaults to
`true` for floors.

### Art
- Transition tilesets for the new `flagstone` / `checkered_marble` floors (`assets/floors/transitions/<low>__<high>/`) — currently they meet terrain with a hard quadrant edge, acceptable indoors.
- The barrel sprite deliberately softens the wall-set projection (a strict 1-floor cylinder under the (-36,-24)px/floor shear reads as a lying log — see `scripts/gen_container_set.py`), so objects stacked on a barrel sit slightly up-left of the drawn lid.

### Map authoring
- Brushes / floods / templates so large layouts are not verbose YAML tile lists (`TileRectangleArea` rects already work, plus the ASCII `tiles:` grid; the gap is interactive paint tooling).
- Add validation for map YAML so invalid object IDs or out-of-bounds placements fail clearly.
- Decide how decorative objects (flowers, etc.) should share tiles with blocking objects through explicit layering rules.
- Decide how stacked map objects render visually once trees, items, and walls can share a tile.
- **Editor save is still lossy for authoring form.** `SpaceOutput` (`src/editor/serializer.rs`) now round-trips authored `id:`, `facing:`, `routine:`, `quantity:` and `permanence:` (guarded by tests over every shipped map), but a save still flattens the compact `tiles:`/`legend:` ASCII grid and `floors:` rects into per-tile coordinate lists, and drops all comments. Hand-authored maps get much bigger and lose their documentation. Re-compacting floors into rects is the cheap half; re-emitting the char grid needs a heuristic for which objects belong in it.
- `contents:` symbolic references (`MapObjectChild::Reference`) and per-item modifiers still cannot round-trip through an editor save — the editor's source of truth is the flattened `Container.slots`, which carries neither.
- **Floor overlap resolution is non-deterministic.** `SpaceDefinition.floors` is a `HashMap<FloorTypeId, FloorPlacements>`, so when two floor rects cover the same tile the winner depends on hash iteration order and can differ between runs. `overworld.yaml`'s header comment claims "cave_floor is listed first so the road wins", but the resolved map has `cave_floor` winning on 17 tiles. Wants a deterministic order (`IndexMap`, or a `Vec` of layers).
- Spawn-group `behavior:` blocks in map YAML accept `step_interval_seconds` / `detect_distance_tiles` / `disengage_distance_tiles`, but `MapBehavior` only has `bounds`, so serde silently ignores them. All 9 `overworld.yaml` groups author them to no effect — either wire them up or delete them.

### Gameplay polish
- Introduce richer collision semantics than a single blocking flag.
- Generalize the new NPC behavior system so mobs/NPCs can share the same behavior component layer.
- Companion mechanic + timed summon spell shipped: `Faction` (PlayerSide/MonsterSide) + `Companion` components, faction-aware NPC targeting (`nearest_visible_enemy`), companion kill credit via `DamageSource::OwnedByPlayer`, and the `summons_creature` spell effect (`summon_wolf`). Deferred follow-ups: (a) a hard owner-distance leash so a companion can't chase an enemy arbitrarily far from its owner (today only the follow-when-idle pull recenters it); (b) an "owned-by-you" visual tint on the client (projection currently sends no owner info); ~~(c) monster-owned companions are supported by the generic path (`Companion.owner_player = None`) but no NPC that summons is authored yet~~ — **shipped**: `spawn_summoned_creature` now takes `owner_player: Option<PlayerId>` + an explicit `Faction`, NPC casts honor `summons_creature` through the `PendingNpcSummons` deferred queue (`apply_pending_npc_summons`), and Cinderjack / The Deeplistener both summon adds; (d) summon cap is hard-coded to 1/owner — no per-spell cap field. Note the cap despawns *pre-existing* companions before the `count` loop, so `count: 3` correctly yields three adds and a recast replaces them.
- Decide how much scripting authority the embedded Python console should keep once server-authoritative logic exists.

## Risks

- Persistence-heavy gameplay will require durable IDs to keep working as item/container counts grow (mostly addressed by the format-v7 multi-space dump, but new persistent systems must not regress this).
- AoI / interest management is not implemented — `compute_events_for_peer` broadcasts everything. Player count above ~5 will saturate bandwidth. Track in `FEATURE_BACKLOG.md`.

## Completed

- Bootstrapped the Bevy project structure and initial app/world/player plugin layout.
- Added a simple colored tile grid and a player marker with explicit tile coordinates.
- Implemented one-tile movement with map-bounds clamping and Tibia-style centered-player scrolling.
- Added starter map features (water patches, tree clusters), a data-driven overworld object format, and ECS collider components.
- Expanded the overworld object catalog (grass, walls, barrels, flowers, stones) with metadata-driven collision.
- Moved default map layout into YAML; placement no longer hardcoded in Rust.
- Added an embedded Python console (RustPython) with world listing and object spawning.
- Added data-driven equippable gear definitions with typed equipment slots.
- Added basic player stats with equipment-driven health, mana, and storage bonuses.
- Added metadata-driven usable consumables with context-menu use actions.
- Added instance-authored roaming NPC behavior with bounded random movement.
- Added a first combat loop with per-character targets, global battle tick, and melee hit log.
- Added a first attribute system (strength/agility/constitution/willpower/charisma/focus) driving derived health, mana, and carrying capacity.
- Added first-pass melee damage so combat turns reduce hit points and can defeat the player.
- Added a hostile roam-and-chase NPC behavior and a first goblin encounter.
- Added first-pass scroll-cast magic with YAML spell defs, untargeted/self-cast and targeted modes, and a spell-target cursor.
- Generalized the right sidebar into docked windows; status, equipment, backpack, target, container panels share the same scrollable/resizable dock.
- Introduced a server-authoritative command layer; gameplay mutations for movement, targeting, item actions, spell casting, drag/drop, and console spawns go through `PendingGameCommands`.
- Allowed right-click context interactions and combat targeting against nearby remote players.
- Made players block movement and occupied-tile placement for other players via the authoritative collider path.
- Added server-side world-state dumping on graceful exit (`Ctrl+C` handling, JSON save) for authoritative players, objects, and runtime registry state.
- Added authored multi-space support with `persistent`/`ephemeral` space definitions, portal travel, shared dungeon instancing per entrance, and same-space snapshot filtering.
- Added a persistent underworld space with cave assets and a two-way overworld portal.
- Added a title screen with splash art, server selection, author credits, connect flow, exit action.
- Made embedded play load and save the same world snapshot path as headless server mode; fixed local combat HP desync from client projection writing over authoritative state.
- **Account-level persistence**: sqlite DB at `~/.local/share/mud2/accounts.db`, Argon2 password hashing, Login/Register protocol, per-character save on disconnect/autosave/exit. Embedded mode uses reserved `account_id = 0`. World snapshot v5 + later — players no longer ride in `WorldStateDump`.
- **TLS** via `rustls` (sync nonblocking, no tokio). Server: `--tls --tls-cert --tls-key`, `--generate-cert` with `dev-self-signed` for self-signed dev pairs. Client: `--tls` (webpki-roots) or `--insecure`, plus `tls://host:port` URL shorthand.
- **Periodic autosave**: `autosave_all_players` runs every 60s in addition to disconnect/exit saves.
- **Multi-space persistence**: world snapshot is now a `Vec<RuntimeSpaceDump>`; format_version bumped to 7.
- **Stacked floors PoC** (Phase 0/1 only): `TilePosition.z`, `FLOOR_Z_STEP`, roof hiding, stair transitions, floor-aware minimap, two-floor authored building. Phase 2/3 (editor selector, ladder/rope/hole, schemas) paused — see Active section.
- **Dialog system**: yarnspinner-driven NPC dialog with `DialogPanel*` UI, dedicated `dialog_node` field on object definitions, and a first authored villager dialog (`assets/dialogs/demo_villager.yarn`).
- **Quest engine**: per-player persistent quest state with Python and Yarn quest scripting (`src/quest/`, `assets/quests/hunter.py`).
- **Ranged combat**: bow / crossbow / arrow / bolt assets, ranged attack profile in object definitions, archer goblin enemy, kiting AI.
- **Minimap** with floor-aware tile/object filtering.
- **Directional movement** + object rotation by player.
- **Map editor** with placement, modal property editing, undo, and YAML serialization (`src/editor/`).
- **Floor-type tiling**: grass/dirt/stone tilesets with corner-aware transitions and tile variants (`src/world/floor_render.rs`, `assets/floors/`); tileset pack/unpack helper script (`scripts/tile_permutor.py`).
- **In-process command pipeline / transport abstraction**: `ServerTransport`/`ClientTransport` wrap raw TCP and TLS streams; embedded mode runs `GameServerPlugin` and `GameClientPlugin` in the same `App` so the wire protocol is bypassed but data flow is identical to networked mode.
- **Decision: stay single-crate.** Networking shipped without splitting `shared/`; module boundaries inside `src/` are sufficient. Revisit only if a real second binary needs a fragment of the code.
- **Spawn pools / respawn groups**: `SpawnGroupDef` in map YAML (`area` + `max_count` + `respawn_mean_seconds`), `tick_spawn_groups` (`src/npc/mod.rs`), editor spawn-groups panel (`src/editor/ui/spawn_groups_panel.rs`).
- **Currency, pouches, carry weight**: copper/silver/gold coin assets with stack-tier sprites, `src/game/currency.rs` (`COPPER_PER_SILVER = 12`, `SILVER_PER_GOLD = 20`), `MaxCarryWeight::from_strength`, pouch base + `PouchInBackpack` docked panel; nested-pouch depth capped to 1 via `accepts_storable_containers: false`.
- **Progression Phase A — XP + Level**: `Experience` component + `xp_for_level` (`src/player/progression.rs`); `ExperienceGained` / `LevelUp` / `ExperienceLost` GameEvents; XP bar (`sync_xp_bar`); `LevelUpToast` GameUiEvent + transient overlay.
- **Progression Phase B — Classes**: `Class` enum + per-class data (`src/player/classes.rs`); `ChooseClass` command + class-picker UI; class-aware `DerivedStats::from_base_with_class`.
- **Progression Phase D — Death penalty**: `drain_inventory_with_drop_chance` (backpack always drops, per-slot equipment roll), XP-zero rule, `GameUiEvent::DeathSummary` + dedicated overlay (`src/ui/systems.rs`).
- **HP / mana regen + food buffs**: `tick_regen_buffs` and `RegenBuffs` (`src/player/regen.rs`); food / drink items grant a temporary regen-rate multiplier.
- **Diagnostics overlay (F3–F12)**: `src/diagnostics/mod.rs` with FPS readout, frame-time min/avg/p99/max, present-mode cycling, archetype histogram, `DiagnosticPause` simulation toggle, render-bisection toggles, and per-frame spike attribution dumps.
- **Camera-based scrolling refactor**: sprites at absolute world coords, `Camera2d` follows the player (`src/world/camera.rs`), conditional Transform writes throughout; `Changed<Transform>` dropped from ~5,500/frame to 1–6/frame.
- **Door states + lever wiring**: `wooden_door` open/closed states, `lever` wires to a target via `side_effects: set_target_state` (`assets/overworld_objects/{wooden_door,lever}/metadata.yaml`).
- **Progression Phase C — Skills + Locks + Social cluster**: `SkillSheet` component + per-class point budget on level-up + `skill_check` helper (`src/player/skills.rs`); `AllocateSkillPoint` command and Skills panel UI (`src/ui/skills_panel.rs`, `KeyK`); skill/key-gated interactions with `pick_lock` / `force_lock` / `use_key` verbs and a `lock:` block on object definitions (`src/world/interactions.rs`); Persuasion-driven vendor price modifier (`vendor_price_for`, `src/game/trade.rs`); Yarn `<<skill_check Skill DC>>` custom command + `skill_rank(name)` library function (`src/dialog/systems.rs`, `src/dialog/yarn_bindings.rs`). See `docs/skills_locks_social_plan.md`.
- **Item hover tooltips** in inventory and equipment slots (`sync_item_tooltip`).
- **`docs/yaml_formats.md` floor schema**: `floors`, tile/rect `z`, `occludes_floor_above`, `walkable_surface`, and the dedicated Floor Transition Metadata section (`§5`) are documented.
- **Object wiring & state machine in YAML**: declarative `states`, `interactions`, `wires_to`, and `side_effects` schema (see `docs/yaml_formats.md`) replace ad-hoc Rust handlers for stateful props.
- **Username/password v1 validation**: length + allowed-char checks via `validate_username` / `validate_password` (`src/accounts/db.rs`).
- **Stealth awareness markers** (player-facing): while sneaking, the player periodically rolls **Perception** (DC scales with distance) to "read" each nearby hostile NPC (`player::sense::tick_player_sense`, `SenseReveals` component). A successful read replicates that NPC's awareness in `ClientWorldObjectState.awareness` (`NpcAwareness` Unaware/Searching/Alerted, computed per-peer in the projection from `is_targeting_local_player` + `AiState`), rendered as a colored over-head glyph — `z`(green)/`?`(yellow)/`!`(red) — by `client_effects::awareness` (reads `ClientGameState`; works in all runtime modes; drawn above the darkness quad). Low Perception → unreliable reads → you sneak partly blind. The classic Metal-Gear `!`/`?` stealth feedback, gated on a skill roll.
- **NPC AI debug overlay** (`Shift+F7` or **Debug ▸ NPC AI** menu, with a live `[X]` checkmark): per-NPC box over the head showing live FSM state (Wander/Alert/Pursue/Engage/Flee + timers), current target, `perception`/`detect`/LoS, and heard-noise tile. `src/npc/debug_overlay.rs`, registered in `DiagnosticsPlugin`. Reads authoritative NPC components, so it only populates in EmbeddedClient mode (the AI-debugging mode). Tracks the NPC's rendered position and draws at absolute z=999.5 — above the darkness quad (z=999) so boxes stay readable at night; backdrop is sized to its text so nothing overflows.
- **Utility systems design + Stealth/Detection slice 1** ("Sneak & Seek"): `docs/utility_systems.md` (shared-signal thesis + 4 interlocking clusters + Medium-sim Exertion). First slice shipped: `SetSneaking` command + `Sneaking` marker + replication + HUD indicator (`V` key); server-side `light_level_at` (`src/world/lighting.rs`); noise bus (`src/world/noise.rs`, emitted by movement/interactions/combat); NPC detection rewritten as an opposed Stealth-vs-Perception roll modified by light + line-of-sight cover, with NPCs investigating heard noise via `AiState::Alert` (`src/npc/detection.rs`, `src/npc/systems.rs`, new `HostileBehavior.perception`); `GameUiEvent::Spotted` cue. **Slice 2 (Exertion + Endurance) shipped:** the Medium-sim stamina currency (`src/player/exertion.rs`, `Exertion` component, fatigue DC via `Dc` stack, regen slowdown), the Endurance regen wire (`src/player/regen.rs`), and the `Concentration→Endurance` rename. **Slice 3 (Physical manipulation / Cluster B) shipped:** weight-gated multi-tile push/pull extending `MoveItem` (`handle_object_push` in `src/game/systems.rs`, `traversal::push_dc`, `EXERTION_COST_PUSH`, `PUSH_NOISE`); objects-as-cover / stack-to-climb / barricade emerge from the existing LoS/stack/A* systems (tests added/identified); and pressure-plate holds (`src/world/pressure_plate.rs` + `pressure_plate:` authoring block) that a shoved heavy object can hold down to drive a wired door. Follow-ups (sketched in the doc): Slice 4 production/quality, Slice 5 knowledge/body, and the deferred Backstab/sneak-attack damage.

## Later Ideas

- Chunk-based world streaming and AoI-based replication.
- Persistent dropped items and containers (today: containers persist; dropped items handled via world-object loot, but ground-item decay timers are not implemented).
- Debug/admin tools for spawning and inspecting entities (today: only Python `world.spawn_object`).
