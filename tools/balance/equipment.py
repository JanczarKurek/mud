"""Real equipment and spell numbers, transcribed verbatim from the game assets.

Sources:
  * weapons / armor / shields: assets/overworld_objects/<id>/metadata.yaml
  * spells:                    assets/spells/<id>.yaml

These are the *actual shipping numbers* (as of this analysis), so the report's
claims trace straight back to data files. Where a weapon has no ``damage:``
field it falls back to ``DamageExpr.melee_default()`` (1d6+strength/5), exactly
as the loader does.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, Optional

from damage_expr import DamageExpr
from stats_model import AttributeSet


@dataclass(frozen=True)
class Weapon:
    id: str
    name: str
    damage: Optional[str]          # None -> melee_default
    kind: str                      # "melee" / "ranged"
    stat_bonus: Dict[str, int] = field(default_factory=dict)   # equipped attribute buffs
    note: str = ""

    def expr(self) -> DamageExpr:
        return DamageExpr.parse(self.damage) if self.damage else DamageExpr.melee_default()


@dataclass(frozen=True)
class ArmorPiece:
    id: str
    name: str
    slot: str
    armor: int = 0
    dodge_bonus: int = 0


@dataclass(frozen=True)
class Shield:
    id: str
    name: str
    block: int
    block_chance: int


@dataclass(frozen=True)
class Spell:
    id: str
    name: str
    mana_cost: float
    damage: float = 0.0
    heal: float = 0.0
    damage_type: str = "physical"
    min_caster_level: int = 0
    classes: tuple = ()
    # Rough extra damage-over-time from an applied effect (magnitude * ticks);
    # labelled an estimate in the report because the tick interval is not exact.
    dot_estimate: float = 0.0
    note: str = ""

    @property
    def damage_per_mana(self) -> Optional[float]:
        total = self.damage + self.dot_estimate
        return total / self.mana_cost if self.mana_cost > 0 and total > 0 else None


# --- weapons (assets/overworld_objects/*/metadata.yaml) -------------------
WEAPONS: Dict[str, Weapon] = {
    "bronze_sword": Weapon("bronze_sword", "Bronze Sword", None, "melee",
                           note="no damage field -> 1d6+strength/5 default"),
    "bow":          Weapon("bow", "Shortbow", "1d6+strength", "ranged",
                           stat_bonus={"agility": 1},
                           note="damage keys off STR, to-hit off AGI (mismatch)"),
    "crossbow":     Weapon("crossbow", "Crossbow", "2d4+agility", "ranged",
                           note="damage and to-hit both key off AGI"),
    "herb_knife":   Weapon("herb_knife", "Herb Knife", "1d3+strength", "melee",
                           note="tool; full STR scaling"),
    "pickaxe":      Weapon("pickaxe", "Pickaxe", "1d4+strength", "melee",
                           note="tool; full STR scaling — out-damages the sword"),
}


# --- armor & shields ------------------------------------------------------
ARMOR: Dict[str, ArmorPiece] = {
    "leather_armor":  ArmorPiece("leather_armor", "Leather Armor", "armor", armor=3),
    "leather_helmet": ArmorPiece("leather_helmet", "Leather Helmet", "helmet", armor=1),
    "leather_legs":   ArmorPiece("leather_legs", "Leather Legs", "legs", armor=2),
    "traveler_boots": ArmorPiece("traveler_boots", "Traveler Boots", "boots", armor=1, dodge_bonus=1),
}

SHIELDS: Dict[str, Shield] = {
    "wooden_shield": Shield("wooden_shield", "Wooden Shield", block=3, block_chance=25),
}

# The only full armor set that ships: every leather piece + boots.
FULL_LEATHER = ["leather_armor", "leather_helmet", "leather_legs", "traveler_boots"]


@dataclass
class Loadout:
    name: str
    weapon: str
    armor: tuple = ()
    shield: Optional[str] = None

    def total_armor(self) -> int:
        return sum(ARMOR[a].armor for a in self.armor)

    def total_dodge(self) -> int:
        return sum(ARMOR[a].dodge_bonus for a in self.armor)

    def stat_bonus(self) -> AttributeSet:
        b = WEAPONS[self.weapon].stat_bonus
        return AttributeSet(b.get("strength", 0), b.get("agility", 0),
                            b.get("constitution", 0), b.get("willpower", 0),
                            b.get("charisma", 0), b.get("focus", 0))


# Named loadouts referenced by adventurers.py.
LOADOUTS: Dict[str, Loadout] = {
    "naked_sword":     Loadout("naked sword", "bronze_sword"),
    "starter_bow":     Loadout("starter bow", "bow"),                       # the literal new-char kit
    "leather_sword":   Loadout("leather + sword", "bronze_sword", tuple(FULL_LEATHER)),
    "leather_pick":    Loadout("leather + pickaxe", "pickaxe", tuple(FULL_LEATHER)),
    "leather_shield":  Loadout("leather + sword + shield", "bronze_sword", tuple(FULL_LEATHER), "wooden_shield"),
    "leather_crossbow": Loadout("leather + crossbow", "crossbow", tuple(FULL_LEATHER)),
}


# --- spells (assets/spells/*.yaml) ----------------------------------------
# Direct-damage and heal spells; pure utility/buff spells are omitted.
SPELLS: Dict[str, Spell] = {
    "magic_dart":     Spell("magic_dart", "Magic Dart", 4.0, damage=5.0,
                            damage_type="arcane", min_caster_level=1, classes=("Wizard",)),
    "spark_bolt":     Spell("spark_bolt", "Spark Bolt", 12.0, damage=18.0,
                            damage_type="lightning", min_caster_level=1, classes=("Wizard",),
                            note="huge flat damage per mana — the efficiency outlier"),
    "fireball_minor": Spell("fireball_minor", "Lesser Fireball", 12.0, damage=8.0,
                            damage_type="fire", min_caster_level=0, classes=(),
                            note="AOE radius 1"),
    "fireball":       Spell("fireball", "Fireball", 22.0, damage=14.0,
                            damage_type="fire", min_caster_level=0, classes=(),
                            note="AOE radius 1; class gate commented out"),
    "frost_lance":    Spell("frost_lance", "Frost Lance", 16.0, damage=7.0,
                            damage_type="frost", min_caster_level=3, classes=("Wizard",),
                            note="+slow"),
    "frost_bolt":     Spell("frost_bolt", "Frost Bolt", 18.0, damage=4.0,
                            damage_type="frost", min_caster_level=4, classes=("Wizard",),
                            dot_estimate=0.0, note="+chill (utility, low direct dmg)"),
    "immolation":     Spell("immolation", "Immolation", 20.0, damage=6.0,
                            damage_type="fire", min_caster_level=5, classes=("Wizard",),
                            dot_estimate=8.0, note="burning 2.0/~10s; DoT is an estimate"),
    "lesser_heal":    Spell("lesser_heal", "Lesser Heal", 8.0, heal=20.0,
                            min_caster_level=1, classes=("Cleric",)),
    "cure_wounds":    Spell("cure_wounds", "Cure Wounds", 14.0, heal=30.0,
                            min_caster_level=3, classes=("Cleric",)),
    "restore":        Spell("restore", "Restore", 28.0, heal=9999.0,
                            min_caster_level=8, classes=("Cleric",),
                            note="full heal"),
}


def _selftest() -> None:
    # Sword falls back to default; pickaxe out-damages it at high STR.
    strong = AttributeSet(strength=18)
    sword = WEAPONS["bronze_sword"].expr().mean_damage(strong)   # 1d6+18/5 = 3.5+3
    pick = WEAPONS["pickaxe"].expr().mean_damage(strong)         # 1d4+18   = 2.5+18
    assert pick > sword + 10, (sword, pick)
    assert LOADOUTS["leather_sword"].total_armor() == 7          # 3+1+2+1
    assert LOADOUTS["leather_sword"].total_dodge() == 1
    assert abs(SPELLS["spark_bolt"].damage_per_mana - 1.5) < 1e-9
    print("equipment selftest OK")


if __name__ == "__main__":
    _selftest()
