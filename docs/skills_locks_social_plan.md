# Lockpicking + Social Cluster — Implementation Plan

## Context

The progression system has shipped Phases A (XP+level), B (classes), D (death
penalty), and E (spell gating + level-scaled caster mana). **Phase C (skills)
is the only remaining piece.** Rather than land `SkillSheet` as pure
infrastructure that no system consumes, this batch ships Phase C *together
with* the first set of in-game systems that actually use skills — proving the
foundation works in real gameplay and giving four classes immediate
build-meaningful payoffs.

Skills covered in this cluster: **Thievery, Athletics (force-door),
Persuasion, Lore**. Class identity unlocked: Vagabond (lockpicking),
Fighter (force door), Cleric/Wizard/Vagabond (dialog branches).

Skills explicitly **not** in this cluster (deferred to later batches because
they require infrastructure that doesn't exist yet):

- **Spellcraft, Concentration** → batch with the Tibia-runes/cast-times work.
- **Athletics climb/jump/swim** → batch with the multistorey-walkable work.
- **Stealth, Perception** → need a `Hidden` component layer that doesn't exist.
- **Survival** → needs hunger system first.
- **Heal** → small, but no driver here; fold into a later "consumables polish" pass.
- **Lore-identify items** → most novel piece in this cluster (per-character
  known-items state, "Strange Bauble" naming). Defer to a Lore-only follow-up
  so we don't bloat this batch.

## Locked-in design decisions

| Decision | Choice |
|---|---|
| Force-door verb | **Parallel** to Pick Lock (both visible when applicable) |
| Persuasion swing | **±20% cap, 2% per Persuasion rank** (max at rank 10) |
| Skill points / level | **`2 + FOC_mod`** (honors `progression.md §5`) |
| Locked containers | **`locked: true` + `lock_id` on existing container metadata** (no new object kind) |
| Cross-class skill cost | 2 points/rank for cross-class, 1 point/rank for class skills (per `progression.md §5.1`) |

## Phases

### Phase 1 — `SkillSheet` foundation (~3–4 days)

**New files:**
- `src/player/skills.rs` — `Skill` enum (10 variants), `SkillSheet` component, `skill_check()` helper, class-skill table.

**Edits:**
- `src/player/mod.rs` — register the new module.
- `src/player/components.rs` — wire `SkillSheet` into the player bundle.
- `src/player/progression.rs:117–156` — extend the `LevelUp` handler to award `2 + focus_mod(attributes)` skill points (clamped ≥ 1).
- `src/game/commands.rs` + `src/game/resources.rs` — new `GameCommand::AllocateSkillPoint { skill, ranks }` and matching `GameEvent::SkillRanksChanged`. Server validates available points + max-rank caps before applying.
- `src/game/systems.rs` — handler for the new command.
- `src/accounts/db.rs` — extend the per-character save blob with `SkillSheet`. Bump player save schema if needed (the world snapshot is unaffected; this is account-side).
- `src/ui/resources.rs` — new `DockedPanelKind::Skills`.
- `src/ui/skills_panel.rs` (new) — list of 10 skills with current rank, +/- buttons, point counter, class-skill highlighting. Mirror the structure of an existing simple panel (e.g. `recipe_book.rs`).

**`skill_check` signature:**
```rust
pub fn skill_check(
    sheet: &SkillSheet,
    attributes: &AttributeSet,
    skill: Skill,
    dc: i32,
    situational: i32,
) -> SkillCheckResult { roll, total, success }
```
Formula per `progression.md §5.1`: `d20 + ranks + ability_mod + situational vs dc`.

**Tests** (in-module): rank caps for class vs cross-class, point spending decrement, `skill_check` math at known rolls.

---

### Phase 2 — Locks + keys (~3–4 days)

**Edits:**
- `src/world/object_definitions.rs` — add `locked: bool`, `lock_id: u32`, `pick_dc: i32`, `force_dc: i32` to the existing container/door state-machine metadata. Defaults: `locked = false`, `lock_id = 0`. Reuse the existing `states` / `interactions` schema.
- `assets/overworld_objects/wooden_door/metadata.yaml` — add an authored `locked` variant (parallel to existing open/closed states, or via a `locked` flag that gates the `closed → open` transition).
- `assets/overworld_objects/iron_key/metadata.yaml` (new) — `lock_id` matching the locked door's.
- `assets/overworld_objects/wooden_chest/metadata.yaml` (or whichever container exists) — opt-in `locked: true` + `lock_id`.
- `src/game/commands.rs` — new commands: `PickLock { target }`, `ForceLock { target }`, `UseKeyOn { target, key_slot }`.
- `src/game/systems.rs` — handlers: validate adjacency, perform `skill_check(Thievery, pick_dc)` or `skill_check(Athletics, force_dc)`, on success flip `locked → false` (transition fires the existing `closed → open` interaction next click). Emit a chat-line `GameEvent` on success/failure.
- `src/ui/context_menu.rs` (or wherever right-click verbs are built) — show "Pick Lock" / "Force Lock" / "Use Key" verbs only when:
  - target is locked
  - player has Thievery/Athletics rank > 0 (Pick/Force) or matching key in inventory (Use Key)
- `assets/maps/overworld.yaml` — one authored locked door + matching key for testing.

**Tests** (`tests/lockpicking.rs`): pick succeeds at high Thievery, fails at low, key always works, force succeeds at high Athletics, locked-door blocks open transition.

---

### Phase 3 — Persuasion price modifier (~1 day)

**Edits:**
- `src/game/trade.rs` — wherever `TradeOfferEntry` prices are computed (vendor-stock pricing). Apply `multiplier = 1.0 - clamp(persuasion_ranks * 0.02, -0.20, 0.20)` to vendor-side prices. Buyer-favorable on buy, seller-favorable on sell. Centralize in a `vendor_price_for(player, base_price, side) -> u32` helper so tests can hit it directly.
- `src/ui/trade.rs` — small annotation under the trade window: "Persuasion: -12%" when modifier is non-zero.

**Tests** (in-module): boundary checks at 0 ranks (no change), 5 ranks (-10%), 10+ ranks (clamped at -20%).

---

### Phase 4 — Yarn `skill_check` hook (~2 days)

**Edits:**
- `src/dialog/yarn_bindings.rs` — register a new custom command:
  ```
  <<skill_check Persuasion 15>>
  ```
  The command performs `skill_check` on the speaking player and writes the
  result into a Yarn variable (e.g. `$last_skill_check`) that branching nodes
  read.
- `src/dialog/variable_storage.rs` — no schema change expected; the variable is set at runtime via the standard `VariableStorage` API.
- `assets/dialogs/demo_villager.yarn` — gate one branch behind a Persuasion check (e.g. talk villager into a discount or extra info). Proof-of-life demo.
- `docs/yaml_formats.md` — document the `<<skill_check>>` command syntax in the dialog section.

**Tests** (`tests/yarn_skill_check.rs`): a synthetic Yarn snippet invokes `skill_check`, asserts the variable reflects the result, asserts branch routing matches.

---

## Critical files (consolidated)

- `src/player/skills.rs` *(new)*
- `src/player/components.rs`
- `src/player/progression.rs`
- `src/game/commands.rs`, `src/game/resources.rs`, `src/game/systems.rs`
- `src/game/trade.rs`
- `src/dialog/yarn_bindings.rs`
- `src/world/object_definitions.rs`
- `src/ui/resources.rs`, `src/ui/skills_panel.rs` *(new)*, `src/ui/context_menu.rs`, `src/ui/trade.rs`
- `src/accounts/db.rs` (player save blob)
- `assets/overworld_objects/{wooden_door,wooden_chest,iron_key}/metadata.yaml`
- `assets/maps/overworld.yaml`
- `assets/dialogs/demo_villager.yarn`
- `docs/yaml_formats.md`

## Verification

Run after each phase:

1. `cargo check && cargo clippy && cargo test` — green.
2. Manual smoke test in `cargo run --bin mud2`:
   - **Phase 1:** level up → see "+N skill points" toast → open Skills panel → spend points → verify class-skill cost is 1, cross-class is 2, max-rank cap enforced. Save & relaunch — ranks persist.
   - **Phase 2:** approach the authored locked door without the key → "Pick Lock" verb visible iff Thievery > 0; "Force Lock" iff Athletics > 0. Pick at low rank → fail. Pick at high rank → succeed → open as normal. Use key → instant unlock. Repeat for the locked chest.
   - **Phase 3:** open vendor trade → note baseline prices → spend points into Persuasion → reopen trade → prices shift. Cap at ±20% past rank 10.
   - **Phase 4:** talk to demo villager → branch only appears / succeeds at sufficient Persuasion ranks.
3. Multiplayer parity: repeat the four smoke tests in TcpClient mode against a `--bin server` instance to confirm the EmbeddedClient invariant holds (skills are server-authoritative; UI consumes events only).
4. After all phases land, update `PLAN.md` to mark Phase C ✅ shipped, and add a Completed entry in `ISSUES.md`.

## Total scope

~2 weeks of focused work. Phase 1 is the load-bearing piece (Phase C
foundation). Phases 2–4 stack on top quickly once the `skill_check` helper
exists. Each phase is independently shippable and testable, so the batch can
be paused between phases if priorities shift.
