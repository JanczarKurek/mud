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
- 2026-08 audit follow-ups (deferred from the big dedup/split round):
  - `mud2-protocol` crate: move the serde wire types (GameCommand 62 variants, GameEvent, GameUiEvent, ClientGameState, OverworldObjectDefinition) below the lib so the 443 serde derive expansions leave the hot crate. Large type closure — needs `pub use` shims at the old paths.
  - `mud2-scripting` crate: rustpython (130 MB rlib + 27 MB proc-macro) is used by 5 files but ui/player/app reach into `scripting::resources` and quest/network into `scripting_api`, so the cut needs PythonConsoleState (state-only) to stay below while the VM moves up.
  - Remaining small dedups: inventory-slot builders ×3 (ui/setup.rs), settings option-row stacks ×2 + stepper rows ×3, three parallel targeting state machines in ui (UseOn/SpellTargeting/ItemTargeting share prologue/epilogue), 8 bespoke drag implementations, editor row/selection color families.
  - `handle_minimap_scroll_wheel` hit-tests the logical cursor against physical node bounds (misses on HiDPI) — spotted during the wheel-scroll dedup; it's zoom control so it was left alone, but the hit-test looks wrong.
  - `handle_move_item` is still a 384-line 2x2 match (pickup/world-to-world/slot-to-slot/slot-to-world) sharing a duplicated pickup and drop-to-world path with `handle_take_from_stack`; splitting it was deferred as higher-risk.
- Wire-side gating of the remaining `Admin*`/internal `GameCommand`s (AdminSpawn, AdminGrantXp, StashMutate, …): a malicious TCP client can still push them directly. The REPL commands (`AdminExec`, `AdminReplReset`, `AdminSetAccountAdmin`) are already gated on the peer's `is_admin` flag — extend the same check to the rest at ingest or in their drainers.
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

### NPC AI (tag hostility + aggro, 2026-08)
Tag-based hostility shipped (`tags` / `hostile_towards` / `flees_from` / `faction` in
templates; `npc/hostility.rs`; guards/wolves/sheep in the overworld), plus
aggro-on-damage (`npc/aggro.rs` — universal self-defense, attacker carried on
`DamageEvent`) and the ranged-flee fix (A* `Budget` vs `NoPath`, flee re-engage).
Follow-ups:
- ~~**Guilt/witness system**~~ — **shipped** (`npc/guilt.rs`): a third hostility
  axis on top of faction+tags. NPC templates declare social `factions:`
  (own interner, own 64-bit budget); harming a member propagates guilt to every
  *live* member via the batched `PendingGuiltEvents` queue. Guilt is stored
  **per NPC** (`KnownGuilty`, keyed by `PlayerId`, persisted in `NpcStateDump`),
  so killing the witnesses buries the evidence and a respawned guard is
  innocent. Tiers: 0–30 nothing / 31–60 refuses to talk or trade / 61+ hostile
  on sight. Earned `+10` per attack (3s per-victim debounce) and `+70` per kill;
  cleared by dying to that faction or paying a `judge:` NPC
  (`GameCommand::PayGuiltFine` + a "Pay Fine" context verb). Guards now punish
  livestock-killers and villager-attackers, and the per-viewer red highlight
  makes a guard look hostile *only to the criminal*. Remaining gaps:
  - Guilt only accrues from damage **you** deal. There is still no *witness*
    model — murdering a lone shepherd out of sight incriminates you exactly as
    much as doing it on the town plaza, and an NPC killed outright never
    reports it (it just stops existing, though its faction already learned).
  - No guilt decay over time, and no per-creature severity weighting (a sheep
    and a magistrate cost the same).
  - The Judge is reachable only in person, which is awkward once the whole town
    is hunting you; a bounty-board or jail alternative would help.
  - `Chatter` is re-derived on fresh spawn but **not** in the snapshot loader,
    so chatty NPCs go silent after a world reload. Pre-existing, unrelated to
    guilt, but noticed while wiring the faction components.

### e2e suite breakage (pre-existing, found 2026-08-10)
Two failures in `tests/` that predate the guilt work — both confirmed by
re-running the suite on a clean `f62358d` checkout:
- Three suites stopped **compiling** when `DamageEvent.attacker` was added in
  `f62358d` (`combat_scoping`, `death_respawn`, `party` construct the struct
  literally). Fixed in passing by adding `attacker: None`; the compile error had
  been masking the two behavioral failures below.
- `death_respawn::death_despatializes_player_and_npc_reaggros_after_respawn`
  fails: an NPC keeps its `CombatTarget` on a de-spatialized (dead) player. The
  equivalent unit test (`npc_drops_aggro_when_player_dies`) passes, so the gap
  is somewhere in the full pipeline, not in `tick_pursue_or_engage`'s
  missing-target branch.
- `multiplayer_transport::two_clients_receive_snapshots_and_see_each_other_move`
  fails **only inside a full `cargo test --workspace` run** (it passes when its
  binary runs alone, and in a 5-binary subset), on an idle machine, at HEAD as
  well as on this branch. The move command reaches the server queue and is
  consumed, but the mover's tile never changes — looks like state accumulating
  across test binaries (leaked listeners / ports), not a logic bug.
- Prey flee only triggers from `Wander`; a sheep mid-routine or a hybrid
  (fight-or-flight) NPC in `Alert` won't spook. Fine for v1 livestock.
- NPC-on-NPC kills grant no XP and no kill-feed line beyond `[X dies]`;
  guard kills are silent from the player's perspective.
- ~~Villager/townsfolk have no identity tags yet~~ — both now carry
  `tags: [humanoid, townsfolk]` and `factions: [emberbrook_town]`.
- A wolf pack can wipe the paddock faster than `respawn_mean_seconds` refills
  it — tune counts/timers after playtest.

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
- **Floor overlap resolution is non-deterministic.** `SpaceDefinition.floors` is a `HashMap<FloorTypeId, FloorPlacements>`, so when two floor rects cover the same tile the winner depends on hash iteration order and can differ between runs. The 180x130 `overworld.yaml` rewrite side-steps this by making every rect disjoint by construction (the plaza is clipped around the tavern and the general store), but the engine hazard is unchanged and other maps may still trip it. Wants a deterministic order (`IndexMap`, or a `Vec` of layers).
- Spawn-group `behavior:` blocks in map YAML accept `step_interval_seconds` / `detect_distance_tiles` / `disengage_distance_tiles`, but `MapBehavior` only has `bounds`, so serde silently ignores them. Stripped from `overworld.yaml` and documented as ignored in `docs/yaml_formats.md`; `underworld.yaml` and the `hollow_bell` maps still author them to no effect — either wire them up or delete them there too.
- **`behavior.bounds` is not range-checked** against the map's `width`/`height` — `validate_spawn_groups` only checks `area.bounds`. An out-of-range roam rectangle yields a silently misbehaving NPC instead of a load-time panic.
- **NPCs have no leash to home.** `RoamBounds` clamps wandering only; `Pursue`/`Alert`/`Flee` ignore it entirely and `disengage_distance_tiles` measures NPC↔target, so a retreating player tows a mob across the whole map and noise (radius 10) pulls wanderers off post. The 180x130 overworld works around this purely with distance — `overworld_keeps_monster_roam_bounds_clear_of_the_village_watch` in `world/map_layout.rs` guards the ≥20-tile invariant. A real `HomeAnchor` (spawn tile + give-up radius) would make the layout constraint unnecessary.

### Gameplay polish
- Introduce richer collision semantics than a single blocking flag.
- Generalize the new NPC behavior system so mobs/NPCs can share the same behavior component layer.
- Companion mechanic + timed summon spell shipped: `Faction` (PlayerSide/MonsterSide) + `Companion` components, faction-aware NPC targeting (`nearest_visible_enemy`), companion kill credit via `DamageSource::OwnedByPlayer`, and the `summons_creature` spell effect (`summon_wolf`). Deferred follow-ups: (a) a hard owner-distance leash so a companion can't chase an enemy arbitrarily far from its owner (today only the follow-when-idle pull recenters it); (b) an "owned-by-you" visual tint on the client (projection currently sends no owner info); ~~(c) monster-owned companions are supported by the generic path (`Companion.owner_player = None`) but no NPC that summons is authored yet~~ — **shipped**: `spawn_summoned_creature` now takes `owner_player: Option<PlayerId>` + an explicit `Faction`, NPC casts honor `summons_creature` through the `PendingNpcSummons` deferred queue (`apply_pending_npc_summons`), and Cinderjack / The Deeplistener both summon adds; (d) summon cap is hard-coded to 1/owner — no per-spell cap field. Note the cap despawns *pre-existing* companions before the `count` loop, so `count: 3` correctly yields three adds and a recast replaces them.
- Decide how much scripting authority the embedded Python console should keep once server-authoritative logic exists.

## Risks

- Persistence-heavy gameplay will require durable IDs to keep working as item/container counts grow (mostly addressed by the format-v7 multi-space dump, but new persistent systems must not regress this).
- AoI / interest management is partial: `compute_events_for_peer` filters dynamic entities by same-space + `INTEREST_RADIUS` (XY only, z ignored), combat chat lines are scoped via `push_chat_line_near`, and broadcast UI events carry an optional space+tile scope applied at the flush (`push_broadcast_near`). New localized effects/messages must use the scoped paths; remaining gaps (z-aware entity pruning, bandwidth at high player counts) tracked in `FEATURE_BACKLOG.md`.

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
- **In-process command pipeline / transport abstraction**: `ServerTransport`/`ClientTransport` wrap raw TCP, TLS, and (since the 2026-08 unification) the in-process loopback byte pipe. Embedded mode runs the server and client plugins in the same `App` connected over the loopback — the full wire protocol (serde framing included) runs in every mode; the old bypass is gone. Frame ordering via `network::sets`; client intent flows through the `ClientPendingCommands` outbox. The in-game Python console is a thin client (`AdminExec`/`ReplOutput`) gated on the accounts DB `is_admin` flag.
- **Decision: stay single-crate.** Networking shipped without splitting `shared/`; module boundaries inside `src/` are sufficient. Revisit only if a real second binary needs a fragment of the code.
- **Spawn pools / respawn groups**: `SpawnGroupDef` in map YAML (`area` + `max_count` + `respawn_mean_seconds`), `tick_spawn_groups` (`src/npc/mod.rs`), editor spawn-groups panel (`src/editor/ui/spawn_groups_panel.rs`).
- **Currency, pouches, carry weight**: copper/silver/gold coin assets with stack-tier sprites, `src/game/currency.rs` (`COPPER_PER_SILVER = 12`, `SILVER_PER_GOLD = 20`), `MaxCarryWeight::from_strength`, pouch base + `PouchInBackpack` docked panel; nested-pouch depth capped to 1 via `accepts_storable_containers: false`.
- **Progression Phase A — XP + Level**: `Experience` component + `xp_for_level` (`src/player/progression.rs`); `ExperienceGained` / `LevelUp` / `ExperienceLost` GameEvents; XP bar (`sync_xp_bar`); `LevelUpToast` GameUiEvent + transient overlay.
- **Progression Phase B — Classes**: `Class` enum + per-class data (`src/player/classes.rs`); `ChooseClass` command + class-picker UI; class-aware `DerivedStats::from_base_with_class`.
- **Progression Phase D — Death penalty**: `drain_inventory_with_drop_chance` (backpack always drops, per-slot equipment roll), XP-zero rule, `GameUiEvent::DeathSummary` + dedicated overlay (`src/ui/systems.rs`).
- **Dead players de-spatialized**: death removes `SpaceResident`/`TilePosition` (respawn click re-inserts them at home), so NPC aggro, detection, AoE, and remote projection drop the body by construction instead of per-system HP checks; `AwaitingRespawn { death_space }` keeps ephemeral dungeons alive meanwhile. Invariant documented in `common_issues.md`; e2e `tests/death_respawn.rs`.
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
- **Party system** (invite → accept → grouped play): server core in `src/game/party.rs` (`Parties` resource, `process_party_commands` in `CommandIntercept`, per-tick `cleanup_invalid_parties` reconciler — the trade three-piece shape); leader-only invite/kick/promote, disband below 2 members, 30 s invite TTL. **Shared XP**: kill grants tagged `XpGrantKind::Kill` (`src/player/progression.rs`) and rewritten by `split_party_xp_grants` into level-weighted largest-remainder shares over a ×(1+0.15·(n−1)) pool, eligibility = alive + same space + ≤30 tiles of the kill (killer always eligible); per-member "You gain N XP" narrator lines replace the killer's solo broadcast. **Replication**: `GameEvent::PartyStateChanged` → `ClientGameState.party` via `emit_party_events` (never vicinity-pruned; emitted before the positionless early-return so dead viewers keep their roster; vitals rounded to whole points). **Client**: party panel (`src/ui/party_panel.rs`, `MountablePanel` id 11, auto-opens on join, dimmed out-of-range rows, leader Kick/Lead buttons), invite popup (`src/ui/party_invite_popup.rs` — Accept/Decline/close-X all answer the server), context-menu "Invite to Party" (no adjacency gate) + "Set Focus", green minimap dots (own `party` revision counter — out-of-radius members aren't covered by `remote_players_rev`), over-head member/focus diamonds (`src/client_effects/party_markers.rs`). E2e: `tests/party.rs`. Drive-bys: trade partner names replicate for real (was `Player {id}` placeholder); `cleanup_invalid_trades` gained the missing `.before(NetServerSend)` edge. Out of scope: party chat (backlog).

## Later Ideas

- Chunk-based world streaming and AoI-based replication.
- Persistent dropped items and containers (today: containers persist; dropped items handled via world-object loot, but ground-item decay timers are not implemented).
- Debug/admin tools for spawning and inspecting entities (today: only Python `world.spawn_object`).
