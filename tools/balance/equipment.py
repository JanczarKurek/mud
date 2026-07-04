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
    # Lowest d20 face that crits (YAML `crit_range`); 20 = nat-20 only.
    crit_threshold: int = 20
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
    damage_expr: Optional[str] = None     # rolled like a weapon, keyed off FOC + level
    heal: float = 0.0
    damage_type: str = "physical"
    min_caster_level: int = 0
    classes: tuple = ()
    # Rough extra damage-over-time from an applied effect (magnitude * ticks);
    # labelled an estimate in the report because the tick interval is not exact.
    dot_estimate: float = 0.0
    note: str = ""

    def expr(self) -> Optional[DamageExpr]:
        return DamageExpr.parse(self.damage_expr) if self.damage_expr else None

    def mean_damage_at(self, attrs: AttributeSet, level: int) -> float:
        """Expected direct damage for a caster with `attrs` at `level` (0 if no expr)."""
        e = self.expr()
        return e.mean_damage(attrs, level=level) if e is not None else 0.0

    def damage_per_mana_at(self, attrs: AttributeSet, level: int) -> Optional[float]:
        if self.mana_cost <= 0:
            return None
        total = self.mean_damage_at(attrs, level) + self.dot_estimate
        return total / self.mana_cost if total > 0 else None


# --- weapons (assets/overworld_objects/*/metadata.yaml) -------------------
# All weapon damage is modifier-based (`str_mod`/`agi_mod`), matching the d20
# to-hit math; dice size + flat bonus carry the tier identity, and real
# weapons add a level growth term (melee /2, ranged /3; tools/fists none).
WEAPONS: Dict[str, Weapon] = {
    "bronze_sword": Weapon("bronze_sword", "Bronze Sword", "1d8+str_mod+level/2", "melee",
                           note="starter martial template"),
    "iron_sword":   Weapon("iron_sword", "Iron Sword", "1d10+str_mod+1+level/2", "melee",
                           note="mid martial tier (vendor)"),
    "steel_sword":  Weapon("steel_sword", "Steel Sword", "2d6+str_mod+2+level/2", "melee",
                           note="high martial tier (loot)"),
    "dagger":       Weapon("dagger", "Dagger", "1d4+str_mod+level/2", "melee",
                           crit_threshold=19,
                           note="light blade; crits 19-20, backstab platform"),
    "bow":          Weapon("bow", "Shortbow", "1d6+agi_mod+level/2", "ranged",
                           stat_bonus={"agility": 1},
                           note="damage and to-hit both key off AGI"),
    "longbow":      Weapon("longbow", "Longbow", "1d8+agi_mod+1+level/2", "ranged",
                           note="mid ranged tier (vendor)"),
    "crossbow":     Weapon("crossbow", "Crossbow", "2d4+agi_mod+1+level/2", "ranged",
                           note="heavier ranged tier"),
    "herb_knife":   Weapon("herb_knife", "Herb Knife", "1d3+str_mod", "melee",
                           note="tool; tiny dice, no growth term"),
    "pickaxe":      Weapon("pickaxe", "Pickaxe", "1d4+str_mod", "melee",
                           note="tool; small dice — never beats a real weapon"),
}


# --- armor & shields ------------------------------------------------------
# Armor subtracts its FULL value post-hit now (was armor // 2), so the shipped
# values are roughly half the old numbers.
ARMOR: Dict[str, ArmorPiece] = {
    "leather_armor":  ArmorPiece("leather_armor", "Leather Armor", "armor", armor=2),
    "leather_helmet": ArmorPiece("leather_helmet", "Leather Helmet", "helmet", armor=1),
    "leather_legs":   ArmorPiece("leather_legs", "Leather Legs", "legs", armor=1),
    "traveler_boots": ArmorPiece("traveler_boots", "Traveler Boots", "boots", armor=0, dodge_bonus=1),
    "chain_helmet":   ArmorPiece("chain_helmet", "Chain Helmet", "helmet", armor=2),
    "chain_armor":    ArmorPiece("chain_armor", "Chain Armor", "armor", armor=3),
    "chain_legs":     ArmorPiece("chain_legs", "Chain Legs", "legs", armor=2),
    "plate_armor":    ArmorPiece("plate_armor", "Plate Armor", "armor", armor=5),
}

SHIELDS: Dict[str, Shield] = {
    "wooden_shield": Shield("wooden_shield", "Wooden Shield", block=3, block_chance=25),
    "tower_shield":  Shield("tower_shield", "Tower Shield", block=6, block_chance=35),
}
# NOTE: tower_shield also carries dodge_bonus -1 in YAML; modelled via the
# TOWER_SHIELD_DODGE constant in loadouts below.
TOWER_SHIELD_DODGE = -1

# Armor sets.
FULL_LEATHER = ["leather_armor", "leather_helmet", "leather_legs", "traveler_boots"]
FULL_CHAIN = ["chain_armor", "chain_helmet", "chain_legs", "traveler_boots"]
PLATE_MIX = ["plate_armor", "chain_helmet", "chain_legs", "traveler_boots"]


@dataclass
class Loadout:
    name: str
    weapon: str
    armor: tuple = ()
    shield: Optional[str] = None

    def total_armor(self) -> int:
        return sum(ARMOR[a].armor for a in self.armor)

    def total_dodge(self) -> int:
        dodge = sum(ARMOR[a].dodge_bonus for a in self.armor)
        if self.shield == "tower_shield":
            dodge += TOWER_SHIELD_DODGE
        return dodge

    def stat_bonus(self) -> AttributeSet:
        b = WEAPONS[self.weapon].stat_bonus
        return AttributeSet(b.get("strength", 0), b.get("agility", 0),
                            b.get("constitution", 0), b.get("willpower", 0),
                            b.get("charisma", 0), b.get("focus", 0))


# Named loadouts referenced by adventurers.py. The chain/plate tiers are the
# ~L6 / ~L12 upgrade paths (vendor and elite-loot respectively).
LOADOUTS: Dict[str, Loadout] = {
    "naked_sword":     Loadout("naked sword", "bronze_sword"),
    "starter_bow":     Loadout("starter bow", "bow"),                       # the literal new-char kit
    "leather_sword":   Loadout("leather + sword", "bronze_sword", tuple(FULL_LEATHER)),
    "leather_pick":    Loadout("leather + pickaxe", "pickaxe", tuple(FULL_LEATHER)),
    "leather_shield":  Loadout("leather + sword + shield", "bronze_sword", tuple(FULL_LEATHER), "wooden_shield"),
    "leather_crossbow": Loadout("leather + crossbow", "crossbow", tuple(FULL_LEATHER)),
    "leather_dagger":  Loadout("leather + dagger", "dagger", tuple(FULL_LEATHER)),
    "chain_iron":      Loadout("chain + iron sword", "iron_sword", tuple(FULL_CHAIN)),
    "chain_iron_shield": Loadout("chain + iron + shield", "iron_sword", tuple(FULL_CHAIN), "wooden_shield"),
    "chain_longbow":   Loadout("chain + longbow", "longbow", tuple(FULL_CHAIN)),
    "chain_crossbow":  Loadout("chain + crossbow", "crossbow", tuple(FULL_CHAIN)),
    "chain_dagger":    Loadout("chain + dagger", "dagger", tuple(FULL_CHAIN)),
    "plate_steel_tower": Loadout("plate + steel + tower", "steel_sword", tuple(PLATE_MIX), "tower_shield"),
}


# --- spells (assets/spells/*.yaml) ----------------------------------------
# Direct-damage and heal spells; pure utility/buff spells are omitted.
# Re-tiered: distinct expression shapes per role, monotonic single-target
# dmg/mana ladder (dart = sustain baseline, spark_bolt = nuke, AoE pays for
# footprint). Heals scale via `wil_mod`/`level` expressions now; the flat
# `heal` numbers here are the neutral-baseline means for reporting.
SPELLS: Dict[str, Spell] = {
    "magic_dart":     Spell("magic_dart", "Magic Dart", 4.0, damage_expr="1d4+foc_mod+level/4",
                            damage_type="arcane", min_caster_level=1, classes=("Wizard",)),
    "spark_bolt":     Spell("spark_bolt", "Spark Bolt", 12.0, damage_expr="3d6+foc_mod*2+level/2",
                            damage_type="lightning", min_caster_level=1, classes=("Wizard",)),
    "fireball_minor": Spell("fireball_minor", "Lesser Fireball", 12.0, damage_expr="1d6+foc_mod+level/3",
                            damage_type="fire", min_caster_level=0, classes=(),
                            note="AOE radius 1"),
    "fireball":       Spell("fireball", "Fireball", 22.0, damage_expr="2d6+foc_mod+level/2",
                            damage_type="fire", min_caster_level=0, classes=(),
                            note="AOE radius 2; class gate commented out"),
    "frost_lance":    Spell("frost_lance", "Frost Lance", 10.0, damage_expr="1d8+foc_mod+level/3",
                            damage_type="frost", min_caster_level=3, classes=("Wizard",),
                            note="+slow (control)"),
    "frost_bolt":     Spell("frost_bolt", "Frost Bolt", 8.0, damage_expr="1d4+foc_mod",
                            damage_type="frost", min_caster_level=4, classes=("Wizard",),
                            dot_estimate=16.0,
                            note="chill DoT carrier: 1+foc_mod/3+level/8 per tick, 8s"),
    "immolation":     Spell("immolation", "Immolation", 10.0, damage_expr="1d4+foc_mod",
                            damage_type="fire", min_caster_level=5, classes=("Wizard",),
                            dot_estimate=30.0,
                            note="burning DoT carrier: 1+foc_mod/3+level/6 per tick, 10s"),
    "chain_spark":    Spell("chain_spark", "Chain Spark", 14.0, damage_expr="1d8+foc_mod+level/3",
                            damage_type="lightning", min_caster_level=4, classes=("Wizard",),
                            note="AOE spiral radius 3"),
    "lesser_heal":    Spell("lesser_heal", "Lesser Heal", 8.0, heal=16.0,
                            min_caster_level=1, classes=("Cleric",),
                            note="scales: 2d8+wil_mod*2+level"),
    "cure_wounds":    Spell("cure_wounds", "Cure Wounds", 14.0, heal=26.0,
                            min_caster_level=3, classes=("Cleric",),
                            note="scales: 3d8+wil_mod*3+level"),
    "restore":        Spell("restore", "Restore", 28.0, heal=9999.0,
                            min_caster_level=8, classes=("Cleric",),
                            note="full heal"),
}


def _selftest() -> None:
    # Modifier-based weapons: the sword's dice beat the pickaxe's at any STR.
    strong = AttributeSet(strength=18)
    sword = WEAPONS["bronze_sword"].expr().mean_damage(strong)   # 1d8(4.5)+mod(4) = 8.5
    pick = WEAPONS["pickaxe"].expr().mean_damage(strong)         # 1d4(2.5)+mod(4) = 6.5
    assert sword > pick, (sword, pick)
    assert abs(sword - 8.5) < 1e-9, sword
    assert LOADOUTS["leather_sword"].total_armor() == 4          # 2+1+1+0 (full-value armor)
    assert LOADOUTS["leather_sword"].total_dodge() == 1

    # Spell damage scales off foc_mod + level; spark_bolt is the nuke tier.
    foc16 = AttributeSet(focus=16)
    sb1 = SPELLS["spark_bolt"].mean_damage_at(foc16, 1)          # 3d6(10.5)+mod*2(6)+0 = 16.5
    assert 14.0 <= sb1 <= 19.0, sb1
    sb10 = SPELLS["spark_bolt"].mean_damage_at(foc16, 10)        # +level/2 = +5 -> 21.5
    assert sb10 > sb1, (sb1, sb10)
    # Single-target dmg/mana ladder: cantrip most efficient per mana, then the
    # nuke, then AoE per-target (AoE pays for the footprint).
    dart = SPELLS["magic_dart"].damage_per_mana_at(foc16, 5)
    spark = SPELLS["spark_bolt"].damage_per_mana_at(foc16, 5)
    fireball = SPELLS["fireball"].damage_per_mana_at(foc16, 5)
    assert dart > spark > fireball, (dart, spark, fireball)
    print("equipment selftest OK")


if __name__ == "__main__":
    _selftest()
