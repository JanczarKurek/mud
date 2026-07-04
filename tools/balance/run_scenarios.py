#!/usr/bin/env python3
"""Balance scenario runner.

Generates, under ``docs/balance/``:
  * adventurers.csv     — every PC build x sampled level, full stat line
  * creatures.csv       — every creature stat block (game + designed)
  * matchups.csv        — closed-form duel for all PC x creature combos, plus
                          Monte-Carlo win-rate/TTK for the level-appropriate set
  * generated_tables.md — the markdown sweep tables the report embeds

Run with no arguments to regenerate everything. ``--selftest`` runs the whole
module chain's selftests. Pure stdlib; deterministic via a fixed RNG seed.
"""

from __future__ import annotations

import csv
import os
import random
from typing import List, Optional

import combat_model as cm
from adventurers import ADVENTURERS, SAMPLE_LEVELS, BY_ID as ADV_BY_ID
from combat_model import Combatant, expected_dps, hit_chance, mean_damage_per_hit, simulate_duel
from creatures import CREATURES
from equipment import SPELLS, WEAPONS, Loadout, LOADOUTS
from stats_model import (AttributeSet, ability_mod, bab_at, derive_stats, hp_per_level,
                         mana_per_level, xp_for_level, xp_grant_for_kill, CLASSES)

OUT_DIR = os.path.normpath(os.path.join(os.path.dirname(__file__), "..", "..", "docs", "balance"))
SEED = 20260615
MC_TRIALS = 2500


def _attrs_row(at: AttributeSet) -> list:
    return [at.strength, at.agility, at.constitution, at.willpower, at.charisma, at.focus]


def _ttk(hp: int, dps: float) -> float:
    return hp / dps if dps > 0 else float("inf")


# --------------------------------------------------------------------------
# CSV: adventurers
# --------------------------------------------------------------------------
def write_adventurers_csv() -> None:
    path = os.path.join(OUT_DIR, "adventurers.csv")
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["id", "name", "class", "level", "STR", "AGI", "CON", "WIL", "CHA", "FOC",
                    "max_hp", "max_mana", "hp_per_level", "mana_per_level",
                    "weapon", "kind", "to_hit", "dmg_min", "dmg_max", "dmg_mean",
                    "armor", "dodge_bonus", "dodge_dc_offered", "block", "block_chance", "note"])
        for adv in ADVENTURERS:
            for lvl in SAMPLE_LEVELS:
                c = adv.combatant(lvl)
                expr = c.damage_expr
                w.writerow([
                    adv.id, adv.name, adv.class_name, lvl, *_attrs_row(c.attributes),
                    c.max_hp, derive_stats(c.attributes, adv.class_name, lvl).max_mana,
                    hp_per_level(c.attributes, adv.class_name),
                    mana_per_level(c.attributes, adv.class_name),
                    LOADOUTS[adv.loadout_id].weapon, c.kind, c.to_hit_bonus(),
                    expr.min_damage(c.attributes, level=lvl),
                    expr.max_damage(c.attributes, level=lvl),
                    round(expr.mean_damage(c.attributes, level=lvl), 2),
                    c.armor, c.dodge_bonus,
                    cm.dodge_dc(lvl, c.attributes.agility, c.dodge_bonus),
                    c.block, c.block_chance, adv.note,
                ])
    print(f"wrote {path}")


# --------------------------------------------------------------------------
# CSV: creatures
# --------------------------------------------------------------------------
def write_creatures_csv() -> None:
    path = os.path.join(OUT_DIR, "creatures.csv")
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["id", "name", "source", "level", "STR", "AGI", "CON", "WIL", "CHA", "FOC",
                    "mean_hp", "hp_expr", "damage_expr", "kind", "to_hit",
                    "dmg_min", "dmg_max", "dmg_mean", "dodge_dc", "armor", "block", "block_chance",
                    "xp_on_kill", "note"])
        for cr in sorted(CREATURES, key=lambda c: (c.level, c.name)):
            c = cr.combatant()
            expr = c.damage_expr
            w.writerow([
                cr.id, cr.name, cr.source, cr.level, *_attrs_row(cr.attrs),
                cr.mean_hp(), cr.hp_expr, cr.damage or "1d4+str_mod", cr.kind,
                c.to_hit_bonus(),
                expr.min_damage(cr.attrs, level=cr.level),
                expr.max_damage(cr.attrs, level=cr.level),
                round(expr.mean_damage(cr.attrs, level=cr.level), 2),
                cm.dodge_dc(cr.level, cr.attrs.agility, 0), cr.armor, cr.block, cr.block_chance,
                xp_grant_for_kill(cr.level), cr.note,
            ])
    print(f"wrote {path}")


# --------------------------------------------------------------------------
# CSV: matchups (closed-form for all + MC for level-appropriate)
# --------------------------------------------------------------------------
def _closest_creature(level: int):
    return min(CREATURES, key=lambda c: (abs(c.level - level), -c.level))


def write_matchups_csv(rng: random.Random) -> List[dict]:
    path = os.path.join(OUT_DIR, "matchups.csv")
    headline: List[dict] = []
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["adv_id", "adv_name", "level", "weapon", "creature_id", "creature_name",
                    "creature_level", "player_hit_pct", "player_dmg_per_hit", "player_dps",
                    "ttk_player_turns", "creature_hit_pct", "creature_dmg_per_hit",
                    "creature_dps", "ttk_creature_turns", "ttk_ratio", "predicted_winner",
                    "mc_player_winrate", "mc_mean_ttk_turns"])
        for adv in ADVENTURERS:
            for lvl in SAMPLE_LEVELS:
                pc = adv.combatant(lvl)
                appropriate = _closest_creature(lvl)
                for cr in sorted(CREATURES, key=lambda c: (c.level, c.name)):
                    mob = cr.combatant()
                    p_hit = hit_chance(pc, mob)
                    p_dph = mean_damage_per_hit(pc, mob)
                    p_dps = expected_dps(pc, mob)
                    c_hit = hit_chance(mob, pc)
                    c_dph = mean_damage_per_hit(mob, pc)
                    c_dps = expected_dps(mob, pc)
                    ttk_p = _ttk(mob.max_hp, p_dps)
                    ttk_c = _ttk(pc.max_hp, c_dps)
                    ratio = ttk_p / ttk_c if ttk_c not in (0, float("inf")) else float("inf")
                    winner = "player" if ttk_p < ttk_c else ("creature" if ttk_c < ttk_p else "tie")

                    mc_wr = mc_ttk = ""
                    if cr.id == appropriate.id:
                        res = simulate_duel(pc, mob, rng, trials=MC_TRIALS)
                        mc_wr = round(res.a_winrate, 3)
                        mc_ttk = round(res.mean_ttk, 1) if res.mean_ttk else ""
                        headline.append({
                            "adv": adv.name, "level": lvl, "creature": cr.name,
                            "creature_level": cr.level, "winrate": res.a_winrate,
                            "mean_ttk": res.mean_ttk, "p_hit": p_hit, "c_hit": c_hit,
                            "ttk_p": ttk_p, "ttk_c": ttk_c,
                        })
                    w.writerow([
                        adv.id, adv.name, lvl, LOADOUTS[adv.loadout_id].weapon, cr.id, cr.name,
                        cr.level, round(p_hit * 100, 1), round(p_dph, 2), round(p_dps, 2),
                        round(ttk_p, 1), round(c_hit * 100, 1), round(c_dph, 2), round(c_dps, 2),
                        round(ttk_c, 1), round(ratio, 2), winner, mc_wr, mc_ttk,
                    ])
    print(f"wrote {path}")
    return headline


# --------------------------------------------------------------------------
# Markdown sweep tables
# --------------------------------------------------------------------------
def _md_table(headers: List[str], rows: List[List]) -> str:
    out = ["| " + " | ".join(headers) + " |",
           "|" + "|".join("---" for _ in headers) + "|"]
    for r in rows:
        out.append("| " + " | ".join(str(x) for x in r) + " |")
    return "\n".join(out)


def _hit_pct(attack_bonus: int, dodge_dc_val: int) -> int:
    # Nat-20 always hits, nat-1 always misses -> hitting faces clamp to [1,19]
    # (a 5% floor and a 5% cap), matching combat_model.hit_chance.
    needed = dodge_dc_val - attack_bonus
    return round(max(1, min(19, 21 - needed)) / 20 * 100)


def build_tables_md(headline: List[dict]) -> str:
    s: List[str] = []
    s.append("# Generated balance tables\n")
    s.append("_Auto-generated by `tools/balance/run_scenarios.py`. Do not edit by hand._\n")

    # 1. Accuracy landscape: attack bonus vs target AGI (level-1 targets).
    s.append("## 1. Hit% — attack bonus vs target dodge DC (L1 targets)\n")
    s.append("Target dodge DC = 10 + (3·level)/4 + AGI_mod (no dodge items); targets here "
             "are level 1, so the level term is 0. A natural 20 always hits "
             "and a natural 1 always misses, so every cell is bounded to a 5% floor and a "
             "5% cap.\n")
    agis = [6, 8, 10, 12, 14, 16, 18]
    headers = ["atk bonus \\\\ target AGI"] + [f"{ag} (DC{cm.dodge_dc(1, ag, 0)})" for ag in agis]
    rows = []
    for ab in range(-1, 6):
        rows.append([f"+{ab}" if ab >= 0 else str(ab)] +
                    [f"{_hit_pct(ab, cm.dodge_dc(1, ag, 0))}%" for ag in agis])
    s.append(_md_table(headers, rows) + "\n")
    s.append("> Attack bonus is now `ability_mod(STR or AGI) + BAB`, so it grows every level "
             "with the class BAB track (plus the every-4-levels ability bump). The nat-20/1 "
             "rule keeps the extremes at 5%/95%.\n")

    # 2. NPC to-hit vs a SAME-LEVEL player: both sides now scale with level.
    s.append("## 2. NPC accuracy — NPC hit% vs a same-level player\n")
    s.append("NPC to-hit = `ability_mod(STR/AGI) + bab_at(track, level)`; here NPC "
             "ability_mod = +2 (a typical bruiser) on the default ¾ BAB track. The player "
             "defends at the same level with dodge DC = `10 + (3·level)/4 + AGI_mod + "
             "items`, so the level terms cancel and hit% stays in a band instead of "
             "saturating at 95% by mid-level.\n")
    player_builds = [("AGI10, no boots", 10, 0), ("AGI16 + boots", 16, 1),
                     ("AGI18 + boots", 18, 1)]
    headers = ["NPC level"] + [name for name, _, _ in player_builds]
    rows = []
    for lvl in [1, 2, 3, 5, 8, 10, 14, 20]:
        npc_bonus = 2 + bab_at("three_quarter", lvl)
        rows.append([lvl] + [f"{_hit_pct(npc_bonus, cm.dodge_dc(lvl, agi, boots))}%"
                             for _, agi, boots in player_builds])
    s.append(_md_table(headers, rows) + "\n")
    s.append("> Same-level accuracy is now roughly flat across the whole 1–20 band: the "
             "dodge DC level term answers the BAB curve, so AGI/dodge investment matters "
             "at every level. Fighting UP levels shifts both terms against you.\n")

    # 3. Armor effectiveness (deterministic reduction = full armor value, floored at 1 dmg).
    s.append("## 3. Armor effectiveness — damage after armor (deterministic full value)\n")
    raws = [3, 6, 10, 15, 25]
    headers = ["armor"] + [f"raw {r}" for r in raws]
    rows = []
    for armor in [0, 1, 2, 3, 5, 7, 10]:
        cells = []
        for r in raws:
            after = max(1.0, r - armor)
            cells.append(f"{after:.0f} ({round((1-after/r)*100)}%)")
        rows.append([armor] + cells)
    s.append(_md_table(headers, rows) + "\n")
    s.append("> Armor subtracts its FULL value — deterministic, no 0-roll, and the item "
             "card means what it says. Shipped armor values are roughly half the old "
             "numbers (full leather set = 4).\n")

    # 4. Shield/block effectiveness.
    s.append("## 4. Block effectiveness (Wooden Shield: block 3, block_chance 25)\n")
    headers = ["defender AGI", "effective block%", "mean reduction / hit"]
    rows = []
    for ag in [8, 10, 12, 16, 18]:
        eff = cm.effective_block_chance_pct(25, ag)
        rows.append([ag, f"{eff}%", f"{eff/100*3:.2f}"])
    s.append(_md_table(headers, rows) + "\n")
    s.append("> A successful block now removes the **full** block value (3), so its mean "
             "reduction is `block_chance% x 3` — block finally does something, scaling with "
             "AGI through the effective-block-chance bonus.\n")

    # 5. Weapon comparison (evaluated at L1 and L10 to show the growth term).
    s.append("## 5. Weapon mean damage by attribute (modifier-based tiers)\n")
    s.append("Weapon damage = dice + `str_mod`/`agi_mod` (+ tier flat) + a level growth "
             "term (`level/2` on all real weapons; tools none). Mean damage shown at "
             "L1 / L10 for each attribute score.\n")
    headers = ["weapon (expr)", "10 (L1/L10)", "14 (L1/L10)", "18 (L1/L10)"]
    rows = []
    for wid in ["bronze_sword", "iron_sword", "steel_sword", "dagger",
                "bow", "longbow", "crossbow", "pickaxe", "herb_knife"]:
        wp = WEAPONS[wid]
        expr = wp.expr()
        cells = []
        for v in (10, 14, 18):
            at = AttributeSet(strength=v, agility=v)
            cells.append(f"{expr.mean_damage(at, level=1):.1f} / "
                         f"{expr.mean_damage(at, level=10):.1f}")
        rows.append([f"{wp.name} ({wp.damage or '1d4+str_mod'})"] + cells)
    s.append(_md_table(headers, rows) + "\n")
    s.append("> Tier order bronze < iron < steel (and bow < longbow) holds at every "
             "attribute score; the **Pickaxe** and **Herb Knife** are tools with no "
             "growth term, so they fall behind every real weapon by mid-level. The "
             "**Dagger** trades dice for a 19-20 crit range and the backstab opener.\n")

    # 6. Magic economy — spell damage now scales with FOC + level.
    s.append("## 6. Magic economy — spell damage scales with Focus + level\n")
    s.append("Spells roll a damage expression keyed off the caster's Focus modifier and "
             "level. Representative caster: FOC 16, WIL 16. Sustained DPS uses the "
             "retuned regen `2 + WIL + FOC/2` per minute (= 26/min ≈ 0.43 mana/s here), "
             "tuned so a cantrip is sustainable every ~10 s.\n")
    caster = AttributeSet(strength=10, agility=10, constitution=8,
                          willpower=16, charisma=10, focus=16)
    mana_regen_per_s = (2 + caster.willpower + caster.focus / 2.0) / 60.0
    headers = ["spell", "mana", "mean dmg L1", "mean dmg L10", "mean dmg L20",
               "dmg/mana L20", "sustained DPS L20"]
    rows = []
    for sid in ["magic_dart", "spark_bolt", "frost_lance", "frost_bolt", "immolation",
                "fireball_minor", "fireball", "chain_spark"]:
        sp = SPELLS[sid]
        m1 = sp.mean_damage_at(caster, 1)
        m10 = sp.mean_damage_at(caster, 10)
        m20 = sp.mean_damage_at(caster, 20)
        dpm = sp.damage_per_mana_at(caster, 20)
        sustained = mana_regen_per_s / sp.mana_cost * m20
        rows.append([sp.name, sp.mana_cost, f"{m1:.1f}", f"{m10:.1f}", f"{m20:.1f}",
                     f"{dpm:.2f}" if dpm else "-", f"{sustained:.2f}"])
    s.append(_md_table(headers, rows) + "\n")
    s.append("> Each spell now has a distinct shape: the **dart** is the sustain baseline "
             "(best dmg/mana), **Spark Bolt** is the per-cast nuke, the frost/fire "
             "carriers deliver scaling DoTs, and the AoEs pay their per-target premium "
             "for the footprint. DoT rows fold the estimated total DoT into dmg/mana.\n")

    # 7. Progression pacing.
    s.append("## 7. Progression — what changes per class as you level\n")
    headers = ["class (sample build)", "stat", "L1", "L5", "L10", "L20"]
    samples = [("Fighter", "str_fighter"), ("Wizard", "wizard"),
               ("Cleric", "cleric"), ("Vagabond", "archer_bow")]
    rows = []
    for cls, adv_id in samples:
        adv = ADV_BY_ID[adv_id]
        combs = {l: adv.combatant(l) for l in (1, 5, 10, 20)}
        hp = [combs[l].max_hp for l in (1, 5, 10, 20)]
        mp = [derive_stats(combs[l].attributes, cls, l).max_mana for l in (1, 5, 10, 20)]
        th = [combs[l].to_hit_bonus() for l in (1, 5, 10, 20)]
        rows.append([f"{cls} ({adv.name})", "max HP", *hp])
        rows.append(["", "max mana", *mp])
        rows.append(["", "to-hit bonus", *[f"+{x}" for x in th]])
    s.append(_md_table(headers, rows) + "\n")
    s.append("> HP, (caster) mana **and** to-hit now move with level: BAB lifts to-hit every "
             "level and the every-4-levels ability bump raises the build's primary attribute, "
             "so weapon accuracy climbs instead of staying flat.\n")

    # 8. XP pacing — kills to next level vs a level-appropriate creature.
    s.append("## 8. XP pacing — kills to reach the next level\n")
    headers = ["level", "XP to next", "level-appropriate creature", "XP/kill", "kills needed"]
    rows = []
    for lvl in [1, 2, 3, 5, 8, 10, 15, 19]:
        to_next = xp_for_level(lvl + 1) - xp_for_level(lvl)
        cr = _closest_creature(lvl)
        per = xp_grant_for_kill(cr.level)
        rows.append([lvl, to_next, f"{cr.name} (L{cr.level})", per,
                     "∞" if per == 0 else (to_next + per - 1) // per])
    s.append(_md_table(headers, rows) + "\n")

    # 9. Headline MC duels.
    s.append("## 9. Headline duels (Monte-Carlo, level-appropriate creature)\n")
    s.append(f"{MC_TRIALS} trials each, simultaneous turns (1 turn = 1 second). "
             "Win = player alive & creature dead.\n")
    headers = ["adventurer", "lvl", "vs creature", "player hit%", "creature hit%",
               "win rate", "mean kill time (s)"]
    rows = []
    for h in headline:
        rows.append([h["adv"], h["level"], f'{h["creature"]} (L{h["creature_level"]})',
                     f'{h["p_hit"]*100:.0f}%', f'{h["c_hit"]*100:.0f}%',
                     f'{h["winrate"]*100:.0f}%',
                     f'{h["mean_ttk"]:.0f}' if h["mean_ttk"] else "—"])
    s.append(_md_table(headers, rows) + "\n")

    return "\n".join(s)


def write_tables_md(headline: List[dict]) -> None:
    path = os.path.join(OUT_DIR, "generated_tables.md")
    with open(path, "w") as f:
        f.write(build_tables_md(headline))
    print(f"wrote {path}")


def main() -> None:
    os.makedirs(OUT_DIR, exist_ok=True)
    rng = random.Random(SEED)
    write_adventurers_csv()
    write_creatures_csv()
    headline = write_matchups_csv(rng)
    write_tables_md(headline)
    print(f"\nDone. {len(headline)} headline MC duels. Outputs in {OUT_DIR}")


def _selftest() -> None:
    import damage_expr, stats_model, equipment, creatures, adventurers
    for m in (damage_expr, stats_model, cm, equipment, creatures, adventurers):
        m._selftest()
    print("run_scenarios: all module selftests OK")


if __name__ == "__main__":
    import sys
    if "--selftest" in sys.argv:
        _selftest()
    else:
        main()
