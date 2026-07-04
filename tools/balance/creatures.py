"""Creature stat blocks.

`source="game"` rows are transcribed verbatim from
assets/overworld_objects/<id>/metadata.yaml. `source="designed"` rows are new
fillers I added to cover the level gaps (4,5,7,9-20) the shipping bestiary
leaves open, so progression pacing can be analysed end to end. The designed
rows follow the same stat-block style and scaling cadence as the real ones.

Creatures do NOT use the player HP formula: they roll an `hp:` expression and a
`damage:` expression against their own attributes. To-hit uses BAB just like
players (`ability_mod + bab_at(track, level)`); a creature carries its own
`bab_track` (default ¾; brutes like the Cyclops use full). We use the *expected*
HP roll for stable comparisons.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import List, Optional

from combat_model import Combatant
from damage_expr import DamageExpr
from stats_model import AttributeSet


@dataclass(frozen=True)
class Creature:
    id: str
    name: str
    level: int
    attrs: AttributeSet
    hp_expr: str
    damage: Optional[str]          # None -> melee_default
    kind: str = "melee"
    damage_type: str = "physical"
    armor: int = 0
    block: int = 0
    block_chance: int = 0
    bab_track: str = "three_quarter"
    source: str = "game"
    note: str = ""

    def mean_hp(self) -> int:
        return round(DamageExpr.parse(self.hp_expr).mean_damage(self.attrs))

    def damage_expr(self) -> DamageExpr:
        return DamageExpr.parse(self.damage) if self.damage else DamageExpr.melee_default()

    def combatant(self) -> Combatant:
        return Combatant(
            name=self.name,
            attributes=self.attrs,
            max_hp=self.mean_hp(),
            damage_expr=self.damage_expr(),
            is_player=False,
            level=self.level,
            kind=self.kind,
            bab_track=self.bab_track,
            armor=self.armor,
            block=self.block,
            block_chance=self.block_chance,
            has_shield=(self.block > 0 or self.block_chance > 0),
            note=self.note,
        )


def A(s, a, c, w, ch, f) -> AttributeSet:   # terse helper, STR/AGI/CON/WIL/CHA/FOC
    return AttributeSet(s, a, c, w, ch, f)


# Budget template (docs: balance retune): mean damage ≈ 2 + 1.3·L as
# `dice + flat`; mean HP ≈ 20 + 11·L (tanky ×1.4, fragile ×0.7); armor ≈ L/3
# for armored types (full-value subtraction now). The damage slope is what
# makes fighting +3-4 levels up deadly while same-level stays mostly safe.
CREATURES: List[Creature] = [
    # ---- shipping bestiary (verbatim) ----
    Creature("rat", "Giant Rat", 1, A(5, 13, 4, 3, 2, 5),
             "1d4+10+constitution*2", "1d4+1", "melee", "pierce"),
    Creature("goblin", "Goblin", 2, A(8, 11, 8, 6, 4, 7),
             "1d6+20+constitution*2", "1d6+1", "melee", "cut"),
    Creature("archer_goblin", "Archer Goblin", 2, A(7, 13, 7, 6, 4, 7),
             "1d8+15+constitution*2", "1d6+1", "ranged", "pierce"),
    Creature("skeleton", "Skeleton", 3, A(7, 8, 6, 5, 2, 6),
             "1d8+30+constitution*3", "1d8+2", "melee", "cut",
             armor=1, block=2, block_chance=15),
    Creature("goblin_mage", "Goblin Mage", 3, A(6, 10, 7, 13, 5, 14),
             "1d6+20+constitution*2", "1d3", "ranged", "arcane",
             note="also casts magic_dart/fireball/sleep/heal — melee profile only here"),
    Creature("fire_elemental", "Fire Elemental", 6, A(12, 13, 12, 14, 6, 10),
             "2d10+40+constitution*3", "2d6+3", "melee", "fire",
             armor=1, note="35% on-hit burning 2.0/6s (not in this melee profile)"),
    Creature("cyclops", "Cyclops", 8, A(18, 5, 16, 7, 4, 7),
             "2d20+50+constitution*5", "2d8+3", "melee", "blunt",
             bab_track="full"),
    Creature("ogre_brute", "Ogre Brute", 10, A(19, 6, 17, 7, 3, 5),
             "2d20+75+constitution*5", "2d8+6", "melee", "blunt",
             armor=2, bab_track="full",
             note="L10 elite; drops chain pieces + iron sword"),
    Creature("dire_wight", "Dire Wight", 12, A(14, 14, 14, 16, 6, 12),
             "3d12+60+constitution*5", "2d10+7", "melee", "death",
             armor=3,
             note="L12 elite; 30% on-hit chill 2.0/5s; drops steel/plate/tower"),

    # ---- designed fillers (same style; flagged) ----
    Creature("wild_boar", "Wild Boar", 4, A(12, 10, 12, 6, 3, 4),
             "1d10+20+constitution*3", "1d8+3", "melee", "pierce",
             source="designed"),
    Creature("orc_brute", "Orc Brute", 5, A(14, 9, 13, 7, 5, 6),
             "2d8+25+constitution*3", "1d10+3", "melee", "cut",
             armor=1, source="designed"),
    Creature("dire_wolf", "Dire Wolf", 7, A(13, 16, 12, 7, 4, 5),
             "2d8+40+constitution*4", "2d6+4", "melee", "pierce",
             source="designed"),
    Creature("ogre", "Ogre", 9, A(19, 6, 17, 7, 4, 5),
             "2d20+60+constitution*5", "2d8+5", "melee", "blunt",
             armor=2, source="designed"),
    Creature("stone_golem", "Stone Golem", 10, A(18, 4, 20, 8, 2, 4),
             "3d20+70+constitution*4", "2d10+4", "melee", "blunt",
             armor=3, source="designed"),
    Creature("wraith", "Wraith", 12, A(12, 16, 14, 16, 8, 14),
             "3d12+75+constitution*4", "2d10+7", "melee", "death",
             armor=2, source="designed"),
    Creature("troll", "Troll", 14, A(20, 8, 20, 8, 4, 5),
             "3d20+90+constitution*6", "3d8+7", "melee", "blunt",
             armor=3, source="designed"),
    Creature("young_dragon", "Young Dragon", 16, A(22, 12, 20, 14, 10, 12),
             "4d20+110+constitution*6", "3d10+6", "melee", "fire",
             armor=5, source="designed"),
    Creature("demon", "Demon", 18, A(22, 16, 20, 18, 14, 16),
             "4d20+80+constitution*5", "3d10+9", "melee", "fire",
             armor=4, block=4, block_chance=25, source="designed"),
    Creature("ancient_dragon", "Ancient Dragon", 20, A(22, 12, 24, 18, 16, 16),
             "4d20+80+constitution*5", "4d8+10", "melee", "fire",
             armor=6, source="designed",
             note="standard L20 budget; true boss variants (2-3x) are future content"),
]

BY_ID = {c.id: c for c in CREATURES}


def _selftest() -> None:
    # mean-HP anchors against the YAML expressions (budget ≈ 20 + 10·L).
    rat = BY_ID["rat"]
    # 1d4(mean2.5) + 10 + CON4*2(8) = 20.5 -> 20 or 21
    assert 20 <= rat.mean_hp() <= 21, rat.mean_hp()
    goblin = BY_ID["goblin"]
    # 1d6(3.5) + 20 + CON8*2(16) = 39.5
    assert 39 <= goblin.mean_hp() <= 40, goblin.mean_hp()
    cyclops = BY_ID["cyclops"]
    # 2d20(21) + 50 + CON16*5(80) = 151
    assert cyclops.mean_hp() == 151, cyclops.mean_hp()
    # NPC to-hit uses BAB; the Cyclops is a full-BAB brute.
    cb = cyclops.combatant()
    assert cb.to_hit_bonus() == 4 + 8, cb.to_hit_bonus()  # ability_mod(STR18)=4 + bab_at("full",8)=8
    # Every creature's mean damage tracks the budget slope (~2 + 1.3·L ±40%).
    for cr in CREATURES:
        mean = cr.damage_expr().mean_damage(cr.attrs, level=cr.level)
        target = 2 + 1.3 * cr.level
        assert 0.4 * target <= mean <= 1.6 * target or cr.id == "goblin_mage", \
            (cr.id, mean, target)
    print("creatures selftest OK")


if __name__ == "__main__":
    _selftest()
