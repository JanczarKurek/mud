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
from adventurers import ADVENTURERS, SAMPLE_LEVELS
from combat_model import Combatant, expected_dps, hit_chance, mean_damage_per_hit, simulate_duel
from creatures import CREATURES
from equipment import SPELLS, WEAPONS, Loadout, LOADOUTS
from stats_model import (AttributeSet, ability_mod, derive_stats, hp_per_level,
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
                    expr.min_damage(c.attributes), expr.max_damage(c.attributes),
                    round(expr.mean_damage(c.attributes), 2),
                    c.armor, c.dodge_bonus,
                    cm.dodge_dc(c.attributes.agility, c.dodge_bonus),
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
                cr.mean_hp(), cr.hp_expr, cr.damage or "1d6+strength/5", cr.kind,
                c.to_hit_bonus(), expr.min_damage(cr.attrs), expr.max_damage(cr.attrs),
                round(expr.mean_damage(cr.attrs), 2),
                cm.dodge_dc(cr.attrs.agility, 0), cr.armor, cr.block, cr.block_chance,
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
    needed = dodge_dc_val - attack_bonus
    return round(max(0, min(20, 21 - needed)) / 20 * 100)


def build_tables_md(headline: List[dict]) -> str:
    s: List[str] = []
    s.append("# Generated balance tables\n")
    s.append("_Auto-generated by `tools/balance/run_scenarios.py`. Do not edit by hand._\n")

    # 1. Accuracy landscape: player attack bonus vs target AGI.
    s.append("## 1. Hit% — player attack bonus vs target dodge DC\n")
    s.append("Target dodge DC = 10 + AGI_mod (no dodge items). d20 has no nat-20/1 rule.\n")
    agis = [6, 8, 10, 12, 14, 16, 18]
    headers = ["atk bonus \\\\ target AGI"] + [f"{ag} (DC{cm.dodge_dc(ag,0)})" for ag in agis]
    rows = []
    for ab in range(-1, 6):
        rows.append([f"+{ab}" if ab >= 0 else str(ab)] +
                    [f"{_hit_pct(ab, cm.dodge_dc(ag, 0))}%" for ag in agis])
    s.append(_md_table(headers, rows) + "\n")
    s.append("> A player's attack bonus is `ability_mod(STR or AGI)` only — capped at "
             "**+4** (attr 18) and never grows with level.\n")

    # 2. NPC auto-hit asymmetry: NPC to-hit grows +1/level.
    s.append("## 2. The level asymmetry — NPC hit% vs the player as NPC level rises\n")
    s.append("NPC to-hit = `ability_mod(STR/AGI) + level`. Here NPC ability_mod = +2 "
             "(a typical bruiser). Player dodge DCs shown are realistic builds.\n")
    pdcs = [("AGI10, no boots", 10), ("AGI16 + boots", cm.dodge_dc(16, 1)),
            ("AGI18 + boots", cm.dodge_dc(18, 1))]
    headers = ["NPC level"] + [name for name, _ in pdcs]
    rows = []
    for lvl in [1, 2, 3, 5, 8, 10, 14, 20]:
        npc_bonus = 2 + lvl
        rows.append([lvl] + [f"{_hit_pct(npc_bonus, dc)}%" for _, dc in pdcs])
    s.append(_md_table(headers, rows) + "\n")
    s.append("> By ~level 8 the average creature hits essentially every swing, regardless "
             "of how much the player invests in AGI.\n")

    # 3. Armor effectiveness (mean reduction = armor/2, floored at 1 dmg).
    s.append("## 3. Armor effectiveness — mean damage after armor (armor reduces 0..N, mean N/2)\n")
    raws = [3, 6, 10, 15, 25]
    headers = ["armor"] + [f"raw {r}" for r in raws]
    rows = []
    for armor in [0, 1, 2, 3, 5, 7, 10]:
        cells = []
        for r in raws:
            after = max(1.0, r - armor / 2.0)
            cells.append(f"{after:.1f} ({round((1-after/r)*100)}%)")
        rows.append([armor] + cells)
    s.append(_md_table(headers, rows) + "\n")
    s.append("> Armor is a *random* 0..N subtract, so its value is N/2 on average. Full "
             "leather (armor 7) halves a 6-damage hit but barely dents a 25-damage one.\n")

    # 4. Shield/block effectiveness.
    s.append("## 4. Block effectiveness (Wooden Shield: block 3, block_chance 25)\n")
    headers = ["defender AGI", "effective block%", "mean reduction / hit"]
    rows = []
    for ag in [8, 10, 12, 16, 18]:
        eff = cm.effective_block_chance_pct(25, ag)
        rows.append([ag, f"{eff}%", f"{eff/100*1.5:.2f}"])
    s.append(_md_table(headers, rows) + "\n")
    s.append("> Even at high AGI a wooden shield removes well under 1 damage per hit on "
             "average — block is currently almost cosmetic.\n")

    # 5. Weapon comparison.
    s.append("## 5. Weapon mean damage by attribute (the sword-vs-tool problem)\n")
    headers = ["weapon (expr)", "STR/AGI 10", "STR/AGI 14", "STR/AGI 18"]
    rows = []
    for wid in ["bronze_sword", "pickaxe", "herb_knife", "bow", "crossbow"]:
        wp = WEAPONS[wid]
        expr = wp.expr()
        cells = []
        for v in (10, 14, 18):
            at = AttributeSet(strength=v, agility=v)
            cells.append(f"{expr.mean_damage(at):.1f}")
        rows.append([f"{wp.name} ({wp.damage or '1d6+STR/5'})"] + cells)
    s.append(_md_table(headers, rows) + "\n")
    s.append("> The **Bronze Sword** scales at `STR/5`; the **Pickaxe** (a tool!) scales at "
             "full `STR` and out-damages every real weapon for a strong character.\n")

    # 6. Magic economy.
    s.append("## 6. Magic economy — damage spells (flat damage, bypasses armor)\n")
    s.append("Spell damage does not scale with FOC/level; only the mana pool grows. "
             "Sustained DPS assumes WIL 16 regen (~5.2 mana/min = 0.087/s).\n")
    headers = ["spell", "mana", "direct dmg", "dmg/mana", "casts @426 mana (L20)", "burst dmg", "sustained DPS"]
    rows = []
    mana_regen_per_s = (2 + 16 / 5) / 60.0
    pool_l20 = derive_stats(AttributeSet(10, 10, 8, 16, 10, 18), "Wizard", 20).max_mana
    for sid in ["magic_dart", "spark_bolt", "fireball_minor", "fireball", "frost_lance", "immolation"]:
        sp = SPELLS[sid]
        casts = int(pool_l20 // sp.mana_cost)
        burst = casts * sp.damage
        sustained = mana_regen_per_s / sp.mana_cost * sp.damage
        dpm = f"{sp.damage_per_mana:.2f}" if sp.damage_per_mana else "-"
        rows.append([sp.name, sp.mana_cost, sp.damage, dpm, casts, round(burst), f"{sustained:.2f}"])
    s.append(_md_table(headers, rows) + "\n")
    s.append(f"> L20 glass wizard mana pool ≈ **{pool_l20}**. Sustained spell DPS is a "
             "fraction of a point — casters are pure burst, then must melee or wait minutes.\n")

    # 7. Progression pacing.
    s.append("## 7. Progression — what actually changes per class as you level\n")
    headers = ["class (sample build)", "stat", "L1", "L5", "L10", "L20"]
    sample = {"Fighter": AttributeSet(16, 12, 14, 10, 10, 10),
              "Wizard": AttributeSet(10, 10, 8, 16, 10, 18),
              "Cleric": AttributeSet(12, 10, 14, 16, 10, 10),
              "Vagabond": AttributeSet(12, 18, 12, 10, 10, 12)}
    rows = []
    for cls, at in sample.items():
        hp = [derive_stats(at, cls, l).max_health for l in (1, 5, 10, 20)]
        mp = [derive_stats(at, cls, l).max_mana for l in (1, 5, 10, 20)]
        rows.append([cls, "max HP", *hp])
        rows.append(["", "max mana", *mp])
        rows.append(["", "to-hit / weapon dmg", "flat", "flat", "flat", "flat"])
    s.append(_md_table(headers, rows) + "\n")
    s.append("> Only HP and (caster) mana move with level. To-hit and weapon damage are "
             "identical at L1 and L20 — the source of the 'numbers feel bad' drift.\n")

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
