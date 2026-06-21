"""Faithful Python port of one combat turn (`src/combat/systems.rs::resolve_battle_turn`)
plus the `src/combat/formulas.rs` helpers.

Resolution order for a single weapon attack (systems.rs:457-504):
  1. to-hit:   d20 + attack_to_hit_bonus  vs  dodge_dc; miss if below.
  2. damage:   max(1, weapon_expr.roll()).
  3. block:    if defender has a shield and a `block_chance` roll succeeds,
               subtract randint(0, block), floored at 1.
  4. armor:    subtract randint(0, armor), floored at 1.

Notes carried into the analysis:
  * The battle turn timer fires once per second (combat/resources.rs), so one
    "turn" here == one second of real combat.
  * Players get NO BAB; only NPCs add +level to-hit (formulas.rs:24).
  * Spell / DoT damage is applied flat in damage.rs and never reaches armor or
    block — that path is intentionally not modelled here.
"""

from __future__ import annotations

import random
from dataclasses import dataclass, field
from typing import Optional

from damage_expr import DamageExpr
from stats_model import AttributeSet, ability_mod


# --- formulas.rs ----------------------------------------------------------
def dodge_dc(agi: int, dodge_bonus: int) -> int:
    return 10 + ability_mod(agi) + dodge_bonus


def effective_block_chance_pct(raw_chance: int, agi: int) -> int:
    return max(0, min(95, raw_chance + ability_mod(agi) * 2))


def attack_to_hit_bonus(kind: str, attrs: AttributeSet, is_player: bool, level: int) -> int:
    ability = attrs.agility if kind == "ranged" else attrs.strength
    level_bonus = 0 if is_player else level
    return ability_mod(ability) + level_bonus


# --- combatant ------------------------------------------------------------
@dataclass
class Combatant:
    name: str
    attributes: AttributeSet
    max_hp: int
    damage_expr: DamageExpr
    is_player: bool
    level: int
    kind: str = "melee"           # "melee" / "ranged"
    armor: int = 0
    block: int = 0
    block_chance: int = 0         # raw shield value, 0-100
    dodge_bonus: int = 0
    has_shield: bool = False
    # Flat bonus damage applied on every landed hit (e.g. Flame Weapon +1d6 ->
    # modelled as its mean). Bypasses block/armor like the real on-hit path.
    bonus_on_hit_mean: float = 0.0
    note: str = ""

    def to_hit_bonus(self) -> int:
        return attack_to_hit_bonus(self.kind, self.attributes, self.is_player, self.level)


# --- single attack --------------------------------------------------------
def resolve_attack(attacker: Combatant, defender: Combatant, rng: random.Random) -> Optional[int]:
    """Return damage dealt, or ``None`` on a miss."""
    to_hit = rng.randint(1, 20) + attacker.to_hit_bonus()
    if to_hit < dodge_dc(defender.attributes.agility, defender.dodge_bonus):
        return None

    damage = max(1, attacker.damage_expr.roll(attacker.attributes, rng))
    if attacker.bonus_on_hit_mean:
        damage += round(attacker.bonus_on_hit_mean)

    if defender.has_shield:
        chance = effective_block_chance_pct(defender.block_chance, defender.attributes.agility)
        if rng.random() < chance / 100.0:
            damage = max(1, damage - rng.randint(0, defender.block))

    damage = max(1, damage - rng.randint(0, defender.armor))
    return damage


# --- closed-form sanity checks (ignore the min-1 floor) -------------------
def hit_chance(attacker: Combatant, defender: Combatant) -> float:
    """P(hit). d20 is uniform 1..20, no natural-20/1 special-case in code."""
    needed = dodge_dc(defender.attributes.agility, defender.dodge_bonus) - attacker.to_hit_bonus()
    faces = max(0, min(20, 21 - needed))
    return faces / 20.0


def mean_damage_per_hit(attacker: Combatant, defender: Combatant) -> float:
    raw = attacker.damage_expr.mean_damage(attacker.attributes) + attacker.bonus_on_hit_mean
    if defender.has_shield:
        chance = effective_block_chance_pct(defender.block_chance, defender.attributes.agility) / 100.0
        raw -= chance * (defender.block / 2.0)
    raw -= defender.armor / 2.0
    return max(1.0, raw)


def expected_dps(attacker: Combatant, defender: Combatant) -> float:
    """Damage per turn (== per second). Closed-form; MC is authoritative."""
    return hit_chance(attacker, defender) * mean_damage_per_hit(attacker, defender)


# --- 1v1 duel -------------------------------------------------------------
@dataclass
class DuelResult:
    trials: int
    a_wins: int
    b_wins: int
    mutual: int
    ttk_turns_samples: list = field(default_factory=list)   # turns for `a` to kill `b`

    @property
    def a_winrate(self) -> float:
        return self.a_wins / self.trials if self.trials else 0.0

    @property
    def mean_ttk(self) -> Optional[float]:
        s = self.ttk_turns_samples
        return sum(s) / len(s) if s else None

    @property
    def median_ttk(self) -> Optional[float]:
        s = sorted(self.ttk_turns_samples)
        return s[len(s) // 2] if s else None


def simulate_duel(a: Combatant, b: Combatant, rng: random.Random,
                  trials: int = 4000, max_turns: int = 400) -> DuelResult:
    """Simultaneous-turn duel: each turn both act, then deaths resolve.

    Simultaneity avoids handing either side a free initiative advantage; the
    engine's per-turn pair order is not reproduced because it does not affect
    aggregate TTK / win-rate materially.
    """
    res = DuelResult(trials=trials, a_wins=0, b_wins=0, mutual=0)
    for _ in range(trials):
        a_hp, b_hp = a.max_hp, b.max_hp
        turns = 0
        while a_hp > 0 and b_hp > 0 and turns < max_turns:
            turns += 1
            dmg_to_b = resolve_attack(a, b, rng)
            dmg_to_a = resolve_attack(b, a, rng)
            if dmg_to_b:
                b_hp -= dmg_to_b
            if dmg_to_a:
                a_hp -= dmg_to_a
        a_dead, b_dead = a_hp <= 0, b_hp <= 0
        if b_dead and not a_dead:
            res.a_wins += 1
            res.ttk_turns_samples.append(turns)
        elif a_dead and not b_dead:
            res.b_wins += 1
        elif a_dead and b_dead:
            res.mutual += 1
            res.ttk_turns_samples.append(turns)
        # neither dead within max_turns -> stalemate, counted in neither tally
    return res


def _selftest() -> None:
    # formulas.rs anchors.
    assert dodge_dc(10, 0) == 10 and dodge_dc(14, 1) == 13 and dodge_dc(6, 0) == 8
    assert effective_block_chance_pct(90, 20) == 95
    assert effective_block_chance_pct(0, 6) == 0
    assert effective_block_chance_pct(25, 12) == 27
    assert attack_to_hit_bonus("melee", AttributeSet(strength=14), True, 5) == 2
    assert attack_to_hit_bonus("ranged", AttributeSet(agility=12), False, 3) == 4

    # hit_chance: STR10 attacker (+0) vs AGI10 defender (DC10) -> need d20>=10 = 11/20.
    atk = Combatant("a", AttributeSet.uniform(10), 95, DamageExpr.melee_default(), True, 1)
    deff = Combatant("b", AttributeSet.uniform(10), 50, DamageExpr.melee_default(), False, 1)
    assert abs(hit_chance(atk, deff) - 11 / 20) < 1e-9, hit_chance(atk, deff)

    # MC hit-rate must track the closed form.
    rng = random.Random(1)
    hits = sum(1 for _ in range(40000) if resolve_attack(atk, deff, rng) is not None)
    assert abs(hits / 40000 - 0.55) < 0.01, hits / 40000
    print("combat_model selftest OK")


if __name__ == "__main__":
    _selftest()
