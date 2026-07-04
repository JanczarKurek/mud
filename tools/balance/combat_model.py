"""Faithful Python port of one combat turn (`src/combat/systems.rs::resolve_battle_turn`)
plus the `src/combat/formulas.rs` helpers.

Resolution order for a single weapon attack (systems.rs:457-504):
  1. to-hit:   d20 + attack_to_hit_bonus  vs  dodge_dc. A natural 20 always hits
               and a natural 1 always misses; otherwise hit if total >= DC.
  2. damage:   max(1, weapon_expr.roll()). If the raw d20 >= the attacker's
               crit threshold (weapon `crit_range`, default 20) the hit is a
               CRITICAL: the expression is rolled twice and summed.
  3. block:    if defender has a shield and a `block_chance` roll succeeds,
               subtract the FULL `block` value, floored at 1.
  4. armor:    subtract the FULL `armor` value (deterministic), floored at 1.

Notes carried into the analysis:
  * The battle turn timer fires once per second (combat/resources.rs), so one
    "turn" here == one second of real combat.
  * To-hit uses BAB for BOTH players and NPCs: `ability_mod(weapon_ability) +
    bab_at(track, level)` (formulas.rs). A player's track comes from their class;
    a creature carries its own `bab_track` (default ¾, brutes like the Cyclops full).
  * Spell / DoT damage is applied flat in damage.rs and never reaches armor or
    block — that path is intentionally not modelled here.
"""

from __future__ import annotations

import random
from dataclasses import dataclass, field
from typing import Optional

from damage_expr import DamageExpr
from stats_model import AttributeSet, ability_mod, bab_at, weapon_focus_bonus


# --- formulas.rs ----------------------------------------------------------
def dodge_dc(level: int, agi: int, dodge_bonus: int) -> int:
    """10 + (3*level)//4 + AGI_mod + item dodge bonus (universal level term)."""
    return 10 + (3 * level) // 4 + ability_mod(agi) + dodge_bonus


def effective_block_chance_pct(raw_chance: int, agi: int) -> int:
    return max(0, min(95, raw_chance + ability_mod(agi) * 2))


def attack_to_hit_bonus(kind: str, attrs: AttributeSet, track: str, level: int,
                        class_name=None) -> int:
    ability = attrs.agility if kind == "ranged" else attrs.strength
    focus = weapon_focus_bonus(class_name, level) if kind == "melee" else 0
    return ability_mod(ability) + bab_at(track, level) + focus


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
    bab_track: str = "three_quarter"
    armor: int = 0
    block: int = 0
    block_chance: int = 0         # raw shield value, 0-100
    dodge_bonus: int = 0
    has_shield: bool = False
    # Flat bonus damage applied on every landed hit (e.g. Flame Weapon +1d6 ->
    # modelled as its mean). Bypasses block/armor like the real on-hit path.
    bonus_on_hit_mean: float = 0.0
    # Lowest raw d20 face that upgrades a landed hit to a critical (double
    # damage roll). 20 = nat-20 only; a dagger carries 19.
    crit_threshold: int = 20
    # Player class name ("Fighter", ...) or None for creatures. Feeds the
    # Fighter Weapon Focus melee to-hit bonus.
    class_name: Optional[str] = None
    # Self-heal loop (Cleric): when hp falls below half, spend a turn casting
    # a heal instead of attacking, while mana lasts. Mirrors lesser_heal spam
    # in real play — without it the stand-and-trade duel undersells the class.
    heal_mean: float = 0.0
    heal_mana_cost: float = 0.0
    mana_max: float = 0.0
    mana_regen_per_turn: float = 0.0
    note: str = ""

    def to_hit_bonus(self) -> int:
        return attack_to_hit_bonus(self.kind, self.attributes, self.bab_track,
                                   self.level, self.class_name)


# --- single attack --------------------------------------------------------
def resolve_attack(attacker: Combatant, defender: Combatant, rng: random.Random) -> Optional[int]:
    """Return damage dealt, or ``None`` on a miss.

    A natural 20 always hits and a natural 1 always misses; otherwise the hit
    lands when ``d20 + to-hit >= dodge_dc``.
    """
    d20 = rng.randint(1, 20)
    total = d20 + attacker.to_hit_bonus()
    dc = dodge_dc(defender.level, defender.attributes.agility, defender.dodge_bonus)
    hit = (d20 == 20) or (d20 != 1 and total >= dc)
    if not hit:
        return None

    damage = max(1, attacker.damage_expr.roll(attacker.attributes, rng, level=attacker.level))
    if d20 >= attacker.crit_threshold:      # critical: roll the expression twice
        damage += max(1, attacker.damage_expr.roll(attacker.attributes, rng,
                                                   level=attacker.level))
    if attacker.bonus_on_hit_mean:
        damage += round(attacker.bonus_on_hit_mean)

    if defender.has_shield:
        chance = effective_block_chance_pct(defender.block_chance, defender.attributes.agility)
        if rng.random() < chance / 100.0:
            damage = max(1, damage - defender.block)

    damage = max(1, damage - defender.armor)
    return damage


# --- closed-form sanity checks (ignore the min-1 floor) -------------------
def hit_chance(attacker: Combatant, defender: Combatant) -> float:
    """P(hit) with the natural-20/1 rule: nat 20 always hits, nat 1 always misses.

    Count faces 2..19 that clear the DC by the modifiers, always add face 20,
    and never count face 1 -> a 5% hit floor and a 5% miss floor.
    """
    dc = dodge_dc(defender.level, defender.attributes.agility, defender.dodge_bonus)
    bonus = attacker.to_hit_bonus()
    hits = 1 + sum(1 for f in range(2, 20) if f + bonus >= dc)
    return hits / 20.0


def crit_chance_given_hit(attacker: Combatant, defender: Combatant) -> float:
    """P(crit | hit): the fraction of hitting d20 faces at or above the crit
    threshold. Face 20 always hits (and always crits when threshold <= 20);
    face 1 never hits."""
    dc = dodge_dc(defender.level, defender.attributes.agility, defender.dodge_bonus)
    bonus = attacker.to_hit_bonus()
    hit_faces = [20] + [f for f in range(2, 20) if f + bonus >= dc]
    crit_faces = [f for f in hit_faces if f >= attacker.crit_threshold]
    return len(crit_faces) / len(hit_faces) if hit_faces else 0.0


def mean_damage_per_hit(attacker: Combatant, defender: Combatant) -> float:
    expr_mean = attacker.damage_expr.mean_damage(attacker.attributes, level=attacker.level)
    # A crit rolls the expression a second time; on-hit bonus riders are NOT doubled.
    raw = expr_mean * (1.0 + crit_chance_given_hit(attacker, defender)) \
        + attacker.bonus_on_hit_mean
    if defender.has_shield:
        chance = effective_block_chance_pct(defender.block_chance, defender.attributes.agility) / 100.0
        raw -= chance * defender.block          # full block value on a successful block
    raw -= defender.armor                        # deterministic full armor value
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
        a_mana, b_mana = a.mana_max, b.mana_max
        turns = 0
        while a_hp > 0 and b_hp > 0 and turns < max_turns:
            turns += 1
            a_mana = min(a.mana_max, a_mana + a.mana_regen_per_turn)
            b_mana = min(b.mana_max, b_mana + b.mana_regen_per_turn)
            # A combatant below half HP with a heal + the mana casts instead
            # of attacking this turn.
            a_heals = (a.heal_mean > 0 and a_hp < a.max_hp * 0.5
                       and a_mana >= a.heal_mana_cost)
            b_heals = (b.heal_mean > 0 and b_hp < b.max_hp * 0.5
                       and b_mana >= b.heal_mana_cost)
            dmg_to_b = None if a_heals else resolve_attack(a, b, rng)
            dmg_to_a = None if b_heals else resolve_attack(b, a, rng)
            if a_heals:
                a_hp = min(a.max_hp, a_hp + a.heal_mean)
                a_mana -= a.heal_mana_cost
            if b_heals:
                b_hp = min(b.max_hp, b_hp + b.heal_mean)
                b_mana -= b.heal_mana_cost
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
    assert dodge_dc(1, 10, 0) == 10 and dodge_dc(1, 14, 1) == 13 and dodge_dc(1, 6, 0) == 8
    # Level term mirrors the ¾ track: L4 -> +3, L8 -> +6, L20 -> +15.
    assert dodge_dc(4, 10, 0) == 13 and dodge_dc(8, 10, 0) == 16 and dodge_dc(20, 10, 0) == 25
    assert effective_block_chance_pct(90, 20) == 95
    assert effective_block_chance_pct(0, 6) == 0
    assert effective_block_chance_pct(25, 12) == 27
    # to-hit now = ability_mod(weapon ability) + bab_at(track, level)
    # (+ Weapon Focus for Fighter melee).
    assert attack_to_hit_bonus("melee", AttributeSet(strength=14), "full", 5) == 2 + 5
    assert attack_to_hit_bonus("ranged", AttributeSet(agility=12), "three_quarter", 3) == 1 + 2
    # Fighter Weapon Focus: melee-only, 1 + level/5 (anchors from classes.rs).
    assert attack_to_hit_bonus("melee", AttributeSet(strength=14), "full", 5,
                               class_name="Fighter") == 2 + 5 + 2
    assert attack_to_hit_bonus("ranged", AttributeSet(agility=14), "full", 5,
                               class_name="Fighter") == 2 + 5
    assert attack_to_hit_bonus("melee", AttributeSet(strength=14), "half", 20,
                               class_name="Wizard") == 2 + 10

    # hit_chance: STR10 attacker (+0) vs AGI10 defender (DC10) -> need d20>=10 = 11/20.
    atk = Combatant("a", AttributeSet.uniform(10), 95, DamageExpr.melee_default(), True, 1)
    deff = Combatant("b", AttributeSet.uniform(10), 50, DamageExpr.melee_default(), False, 1)
    assert abs(hit_chance(atk, deff) - 11 / 20) < 1e-9, hit_chance(atk, deff)

    # MC hit-rate must track the closed form.
    rng = random.Random(1)
    hits = sum(1 for _ in range(40000) if resolve_attack(atk, deff, rng) is not None)
    assert abs(hits / 40000 - 0.55) < 0.01, hits / 40000

    # Crits: default threshold 20 -> P(crit|hit) = 1/hit_faces. The anchor
    # attacker hits on 10..20 = 11 faces -> 1/11.
    assert abs(crit_chance_given_hit(atk, deff) - 1 / 11) < 1e-9
    # A dagger-style 19-20 range doubles the crit faces.
    atk19 = Combatant("a19", AttributeSet.uniform(10), 95, DamageExpr.melee_default(),
                      True, 1, crit_threshold=19)
    assert abs(crit_chance_given_hit(atk19, deff) - 2 / 11) < 1e-9
    # MC mean damage per landed hit must track the closed form (no mitigation).
    rng = random.Random(7)
    landed = [d for _ in range(60000) if (d := resolve_attack(atk19, deff, rng)) is not None]
    mc_mean = sum(landed) / len(landed)
    cf_mean = mean_damage_per_hit(atk19, deff)
    assert abs(mc_mean - cf_mean) < 0.1, (mc_mean, cf_mean)
    print("combat_model selftest OK")


if __name__ == "__main__":
    _selftest()
