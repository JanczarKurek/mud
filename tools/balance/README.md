# Balance simulation toolkit

A standalone (stdlib-only) Python port of the game's combat & stats math, used to
analyse how character / equipment / creature numbers actually play out. It does
**not** touch the game — it reads nothing at runtime and writes only into
`docs/balance/`.

The written analysis lives in [`docs/balance/report.md`](../../docs/balance/report.md).

## Run

```bash
cd tools/balance
python3 run_scenarios.py --selftest   # verify the port matches the Rust unit tests
python3 run_scenarios.py              # regenerate docs/balance/*.csv and generated_tables.md
```

No dependencies, no virtualenv. (Develop inside `nix-shell` as usual; system
`python3` is fine.)

## Files

| module | mirrors | responsibility |
|---|---|---|
| `damage_expr.py` | `src/combat/damage_expr.rs` | parse/roll `"1d6+strength/5"` expressions |
| `stats_model.py` | `player/components.rs`, `classes.rs`, `progression.rs`, `regen.rs` | attributes, classes, derived HP/mana, XP curve, BAB/saves, regen |
| `combat_model.py` | `combat/formulas.rs`, `combat/systems.rs::resolve_battle_turn` | one attack turn, 1v1 duel, closed-form hit%/DPS/TTK |
| `equipment.py` | `assets/overworld_objects/*`, `assets/spells/*` | real weapon/armor/shield/spell numbers + named loadouts |
| `creatures.py` | `assets/overworld_objects/*` | creature stat blocks (real, verbatim + designed fillers) |
| `adventurers.py` | character-creation point-buy rules | example PC archetypes (all legal builds) |
| `run_scenarios.py` | — | generates the CSVs + markdown tables |

## Fidelity

Each module has a `_selftest()` that asserts its formulas against the exact
constants in the corresponding Rust `#[cfg(test)]` blocks (ability_mod anchors,
XP curve, BAB/saves, derived HP/mana, dodge DC, block clamp, weapon ranges). If
the game's math changes, `python3 run_scenarios.py --selftest` will fail and tell
you the port has drifted.

### Known simplifications

- RNG uses Python's `random` (uniform), not the engine's nanosecond stream — the
  distribution is what matters, not the exact sequence.
- Duels are stand-and-trade: no movement/kiting, no mid-fight regen / potions /
  buffs, single target. Creature HP uses the expected roll.
- Spell/DoT effects are summarised by direct damage (+ a flat DoT estimate);
  the full status-effect engine is not ported.

See `docs/balance/report.md` §5 for how these caveats affect the conclusions.

## Tuning workflow

To test a proposed change (e.g. "what if the Bronze Sword were `1d8+strength/2`?"),
edit the relevant number in `equipment.py` / `creatures.py` / `stats_model.py`,
re-run, and diff the regenerated tables. Because the rosters are plain data, this
is a fast loop for sanity-checking a balance pass before touching Rust/YAML.
