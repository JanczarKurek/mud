"""Example adventurer builds (all legal character-creation point-buy spreads).

Every build spends exactly 12 points over the six attributes, each in [8,18]
(components.rs point-buy rules). They span the archetypes a new player is likely
to roll — including the "I just bumped everything" generalist and a couple of
deliberate min-max traps — so the report can speak to what a fresh character
actually feels like.

A player's offence now grows with level on two axes: BAB (from the class track)
lifts to-hit every level, and an ability bump every 4 levels (4/8/12/16/20)
raises the build's primary offence attribute. HP/mana still scale too. The
matchup sims expose how that growth races the bestiary.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import List, Optional

from combat_model import Combatant
from damage_expr import DamageExpr
from equipment import LOADOUTS, WEAPONS, Loadout
from stats_model import (AttributeSet, CLASSES, bumps_for_level, derive_stats,
                         mana_regen_seconds_per_point)

SAMPLE_LEVELS = (1, 5, 10, 20)


@dataclass(frozen=True)
class Adventurer:
    id: str
    name: str
    class_name: str
    attrs: AttributeSet
    loadout_id: str
    bump_attr: str = "strength"     # primary offence attr; gets the every-4-levels bumps
    # Gear progression: ((min_level, loadout_id), ...) — the build swaps to the
    # highest tier it has reached. Models a player actually buying/looting
    # upgrades instead of swinging the starter kit at L20.
    upgrades: tuple = ()
    note: str = ""

    def loadout_for(self, level: int) -> str:
        best = self.loadout_id
        for min_level, lid in self.upgrades:
            if level >= min_level:
                best = lid
        return best

    def effective_attrs(self, loadout: Loadout, level: int = 1) -> AttributeSet:
        b = loadout.stat_bonus()
        bumps = bumps_for_level(level)
        bump = {self.bump_attr: bumps}
        return AttributeSet(
            self.attrs.strength + b.strength + bump.get("strength", 0),
            self.attrs.agility + b.agility + bump.get("agility", 0),
            self.attrs.constitution + b.constitution + bump.get("constitution", 0),
            self.attrs.willpower + b.willpower + bump.get("willpower", 0),
            self.attrs.charisma + b.charisma + bump.get("charisma", 0),
            self.attrs.focus + b.focus + bump.get("focus", 0),
        )

    def combatant(self, level: int, loadout_id: Optional[str] = None) -> Combatant:
        from equipment import SHIELDS
        loadout = LOADOUTS[loadout_id or self.loadout_for(level)]
        eff = self.effective_attrs(loadout, level)
        derived = derive_stats(eff, self.class_name, level)
        weapon = WEAPONS[loadout.weapon]
        c = Combatant(
            name=f"{self.name} L{level}",
            attributes=eff,
            max_hp=derived.max_health,
            damage_expr=weapon.expr(),
            is_player=True,
            level=level,
            kind=weapon.kind,
            bab_track=CLASSES[self.class_name].bab_track,
            armor=loadout.total_armor(),
            dodge_bonus=loadout.total_dodge(),
            has_shield=loadout.shield is not None,
            crit_threshold=weapon.crit_threshold,
            class_name=self.class_name,
            note=self.note,
        )
        if loadout.shield is not None:
            sh = SHIELDS[loadout.shield]
            c.block = sh.block
            c.block_chance = sh.block_chance
        # Clerics self-heal mid-fight (lesser_heal: 2d8+wil_mod*2+level for
        # 8 mana) — the duel loop spends a turn casting below half HP.
        if self.class_name == "Cleric":
            heal = DamageExpr.parse("2d8+wil_mod*2+level")
            c.heal_mean = heal.mean_damage(eff, level=level)
            c.heal_mana_cost = 8.0
            c.mana_max = float(derived.max_mana)
            c.mana_regen_per_turn = 1.0 / mana_regen_seconds_per_point(eff)
        return c


def a(s, ag, c, w, ch, f) -> AttributeSet:    # STR/AGI/CON/WIL/CHA/FOC
    return AttributeSet(s, ag, c, w, ch, f)


ADVENTURERS: List[Adventurer] = [
    Adventurer("generalist", "Generalist", "Fighter", a(12, 12, 12, 12, 12, 12),
               "leather_sword", bump_attr="strength",
               upgrades=((8, "chain_iron"),),
               note="the 'just bump everything' new player; upgrades late"),
    Adventurer("str_fighter", "STR Fighter", "Fighter", a(16, 12, 14, 10, 10, 10),
               "leather_shield", bump_attr="strength",
               upgrades=((6, "chain_iron_shield"), (12, "plate_steel_tower")),
               note="sword + shield front-liner; buys chain at 6, loots plate at 12"),
    Adventurer("tank", "Tank Fighter", "Fighter", a(12, 12, 18, 10, 10, 10),
               "leather_shield", bump_attr="constitution",
               upgrades=((6, "chain_iron_shield"), (12, "plate_steel_tower")),
               note="max CON, sword + shield"),
    Adventurer("brute", "Brute (pickaxe)", "Fighter", a(18, 10, 16, 8, 10, 10),
               "leather_pick", bump_attr="strength",
               note="dumps WIL; never upgrades the tool — the trap build"),
    Adventurer("archer_bow", "Archer (bow)", "Vagabond", a(12, 18, 12, 10, 10, 10),
               "starter_bow", bump_attr="agility",
               upgrades=((6, "chain_longbow"),),
               note="the literal starter kit; upgrades to longbow + chain"),
    Adventurer("archer_xbow", "Archer (crossbow)", "Vagabond", a(12, 18, 12, 10, 10, 10),
               "leather_crossbow", bump_attr="agility",
               upgrades=((6, "chain_crossbow"),),
               note="same build, crossbow scales dmg with AGI"),
    Adventurer("wizard", "Glass Wizard", "Wizard", a(10, 10, 8, 16, 10, 18),
               "naked_sword", bump_attr="focus",
               note="fragile caster; melee shown is the fallback only"),
    Adventurer("cleric", "Balanced Cleric", "Cleric", a(12, 10, 14, 16, 10, 10),
               "leather_sword", bump_attr="willpower",
               upgrades=((8, "chain_iron_shield"),),
               note="mid martial + heals"),
    Adventurer("skirmisher", "Vagabond Skirmisher", "Vagabond", a(12, 16, 12, 10, 10, 12),
               "leather_dagger", bump_attr="agility",
               upgrades=((6, "chain_dagger"),),
               note="dagger rogue: crit 19-20; backstab openers not modelled"),
]

BY_ID = {adv.id: adv for adv in ADVENTURERS}


def _selftest() -> None:
    for adv in ADVENTURERS:
        adv.attrs.validate_point_buy()    # every build must be legal
    # STR Fighter L1 with sword+shield: HP from base attrs, shield wired.
    c = BY_ID["str_fighter"].combatant(1)
    assert c.has_shield and c.block == 3 and c.block_chance == 25
    # HP = 35 + CON14*6 + STR16*2 = 35+84+32 = 151
    assert c.max_hp == 151, c.max_hp
    # Archer with bow gets +1 AGI from the weapon (18 -> 19).
    bow = BY_ID["archer_bow"].combatant(1)
    assert bow.attributes.agility == 19, bow.attributes.agility
    print("adventurers selftest OK")


if __name__ == "__main__":
    _selftest()
