"""Example adventurer builds (all legal character-creation point-buy spreads).

Every build spends exactly 12 points over the six attributes, each in [8,18]
(components.rs point-buy rules). They span the archetypes a new player is likely
to roll — including the "I just bumped everything" generalist and a couple of
deliberate min-max traps — so the report can speak to what a fresh character
actually feels like.

A player's offence (to-hit + weapon damage) comes only from attributes + gear;
attributes never rise on level-up and players get no BAB, so the *only* thing
that changes across levels here is HP/mana. That asymmetry is the headline the
matchup sims expose.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import List, Optional

from combat_model import Combatant
from equipment import LOADOUTS, WEAPONS, Loadout
from stats_model import AttributeSet, derive_stats

SAMPLE_LEVELS = (1, 5, 10, 20)


@dataclass(frozen=True)
class Adventurer:
    id: str
    name: str
    class_name: str
    attrs: AttributeSet
    loadout_id: str
    note: str = ""

    def effective_attrs(self, loadout: Loadout) -> AttributeSet:
        b = loadout.stat_bonus()
        return AttributeSet(
            self.attrs.strength + b.strength,
            self.attrs.agility + b.agility,
            self.attrs.constitution + b.constitution,
            self.attrs.willpower + b.willpower,
            self.attrs.charisma + b.charisma,
            self.attrs.focus + b.focus,
        )

    def combatant(self, level: int, loadout_id: Optional[str] = None) -> Combatant:
        from equipment import SHIELDS
        loadout = LOADOUTS[loadout_id or self.loadout_id]
        eff = self.effective_attrs(loadout)
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
            armor=loadout.total_armor(),
            dodge_bonus=loadout.total_dodge(),
            has_shield=loadout.shield is not None,
            note=self.note,
        )
        if loadout.shield is not None:
            sh = SHIELDS[loadout.shield]
            c.block = sh.block
            c.block_chance = sh.block_chance
        return c


def a(s, ag, c, w, ch, f) -> AttributeSet:    # STR/AGI/CON/WIL/CHA/FOC
    return AttributeSet(s, ag, c, w, ch, f)


ADVENTURERS: List[Adventurer] = [
    Adventurer("generalist", "Generalist", "Fighter", a(12, 12, 12, 12, 12, 12),
               "leather_sword", "the 'just bump everything' new player"),
    Adventurer("str_fighter", "STR Fighter", "Fighter", a(16, 12, 14, 10, 10, 10),
               "leather_shield", "sword + shield front-liner"),
    Adventurer("tank", "Tank Fighter", "Fighter", a(12, 12, 18, 10, 10, 10),
               "leather_shield", "max CON, sword + shield"),
    Adventurer("brute", "Brute (pickaxe)", "Fighter", a(18, 10, 16, 8, 10, 10),
               "leather_pick", "dumps WIL; wields the high-scaling pickaxe"),
    Adventurer("archer_bow", "Archer (bow)", "Vagabond", a(12, 18, 12, 10, 10, 10),
               "starter_bow", "the literal starter kit; bow dmg keys off STR"),
    Adventurer("archer_xbow", "Archer (crossbow)", "Vagabond", a(12, 18, 12, 10, 10, 10),
               "leather_crossbow", "same build, crossbow scales dmg with AGI"),
    Adventurer("wizard", "Glass Wizard", "Wizard", a(10, 10, 8, 16, 10, 18),
               "naked_sword", "fragile caster; melee shown is the fallback only"),
    Adventurer("cleric", "Balanced Cleric", "Cleric", a(12, 10, 14, 16, 10, 10),
               "leather_sword", "mid martial + heals"),
    Adventurer("skirmisher", "Vagabond Skirmisher", "Vagabond", a(12, 16, 12, 10, 10, 12),
               "leather_sword", "AGI/FOC rogue; backstab not modelled"),
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
