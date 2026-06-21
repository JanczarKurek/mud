"""Faithful Python port of `src/combat/damage_expr.rs`.

`DamageExpr` parses strings such as ``"1d6+strength/5"`` or ``"2d20+80+constitution*6"``
and rolls them against an :class:`AttributeSet`. The same type backs both weapon
damage and NPC ``hp:`` expressions in the game, so this module is reused by both
the combat model and the creature builder.

Rust semantics that matter for parity:
  * integer division truncates toward zero (``i32 / i32``), reproduced by `_idiv`;
  * a term is a die only if it matches ``<digits>d<digits>`` (only one allowed);
  * a bare integer term is an additive bonus;
  * any other term is an attribute, optionally ``*mult`` or ``/div``.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional, Tuple

from stats_model import AttributeSet


def _idiv(a: int, b: int) -> int:
    """Integer division truncating toward zero, matching Rust's ``i32 / i32``."""
    if b == 0:
        return 0
    q = abs(a) // abs(b)
    return -q if (a < 0) != (b < 0) else q


_ATTR_ALIASES = {
    "strength": "strength", "str": "strength",
    "agility": "agility", "agi": "agility",
    "constitution": "constitution", "con": "constitution",
    "willpower": "willpower", "wil": "willpower",
    "charisma": "charisma", "cha": "charisma",
    "focus": "focus", "foc": "focus",
}


@dataclass(frozen=True)
class StatTerm:
    kind: str          # canonical attribute name
    multiplier: int = 1
    divisor: int = 1

    def value(self, attrs: AttributeSet) -> int:
        raw = getattr(attrs, self.kind) * self.multiplier
        return _idiv(raw, self.divisor)


@dataclass
class DamageExpr:
    dice: Optional[Tuple[int, int]] = None     # (count, sides)
    stats: List[StatTerm] = field(default_factory=list)
    bonus: int = 0

    # --- construction -----------------------------------------------------
    @staticmethod
    def melee_default() -> "DamageExpr":
        """Default unarmed/plain-weapon melee: ``1d6 + strength/5``."""
        return DamageExpr(dice=(1, 6), stats=[StatTerm("strength", 1, 5)], bonus=0)

    @staticmethod
    def parse(raw: str) -> "DamageExpr":
        trimmed = raw.strip()
        if not trimmed:
            raise ValueError("empty damage expression")

        dice: Optional[Tuple[int, int]] = None
        stats: List[StatTerm] = []
        bonus = 0

        for raw_term in trimmed.split("+"):
            term = raw_term.strip()
            if not term:
                raise ValueError(f"empty term in '{raw}'")

            lower = term.lower()
            if "d" in lower:
                lhs, _, rhs = lower.partition("d")
                if lhs.isdigit() and rhs.isdigit():
                    if dice is not None:
                        raise ValueError(f"multiple dice terms in '{raw}'")
                    count, sides = int(lhs), int(rhs)
                    if count == 0 or sides == 0:
                        raise ValueError(f"dice must be non-zero in '{raw}'")
                    dice = (count, sides)
                    continue

            # Bare integer -> additive bonus.
            try:
                bonus += int(term)
                continue
            except ValueError:
                pass

            # Attribute term, optionally scaled.
            multiplier, divisor = 1, 1
            if "*" in term:
                stat_part, _, rhs = term.partition("*")
                multiplier = int(rhs.strip())
            elif "/" in term:
                stat_part, _, rhs = term.partition("/")
                divisor = int(rhs.strip())
                if divisor == 0:
                    raise ValueError(f"zero divisor in '{raw}'")
            else:
                stat_part = term

            key = _ATTR_ALIASES.get(stat_part.strip().lower())
            if key is None:
                raise ValueError(f"unrecognized term '{term}' in '{raw}'")
            stats.append(StatTerm(key, multiplier, divisor))

        return DamageExpr(dice=dice, stats=stats, bonus=bonus)

    # --- evaluation -------------------------------------------------------
    def stat_total(self, attrs: AttributeSet) -> int:
        return sum(term.value(attrs) for term in self.stats)

    def roll(self, attrs: AttributeSet, rng) -> int:
        dice_total = 0
        if self.dice:
            count, sides = self.dice
            dice_total = sum(rng.randint(1, sides) for _ in range(count))
        return dice_total + self.stat_total(attrs) + self.bonus

    def min_damage(self, attrs: AttributeSet) -> int:
        dice_total = self.dice[0] if self.dice else 0          # every die shows 1
        return dice_total + self.stat_total(attrs) + self.bonus

    def max_damage(self, attrs: AttributeSet) -> int:
        dice_total = self.dice[0] * self.dice[1] if self.dice else 0
        return dice_total + self.stat_total(attrs) + self.bonus

    def mean_damage(self, attrs: AttributeSet) -> float:
        """Expected roll (no min-1 floor; the floor lives in the combat model)."""
        dice_mean = 0.0
        if self.dice:
            count, sides = self.dice
            dice_mean = count * (sides + 1) / 2.0
        return dice_mean + self.stat_total(attrs) + self.bonus

    def describe(self) -> str:
        parts = []
        if self.dice:
            parts.append(f"{self.dice[0]}d{self.dice[1]}")
        for t in self.stats:
            s = t.kind[:3].upper()
            if t.multiplier != 1:
                s += f"*{t.multiplier}"
            if t.divisor != 1:
                s += f"/{t.divisor}"
            parts.append(s)
        if self.bonus:
            parts.append(str(self.bonus))
        return "+".join(parts) if parts else "0"


def _selftest() -> None:
    from stats_model import AttributeSet as A

    # Parser parity with the Rust unit tests.
    e = DamageExpr.parse("1d6+strength/5")
    assert e.dice == (1, 6) and e.stats[0].divisor == 5 and e.bonus == 0
    assert DamageExpr.parse("2d4+agility").dice == (2, 4)
    bow = DamageExpr.parse("1d6+strength")
    assert bow.stats[0].divisor == 1
    multi = DamageExpr.parse("1d4+agility*2+3")
    assert multi.stats[0].multiplier == 2 and multi.bonus == 3
    hp = DamageExpr.parse("2d20+80+constitution*6")
    assert hp.dice == (2, 20) and hp.bonus == 80 and hp.stats[0].multiplier == 6

    for bad in ("", "1d6++", "1d6+luck"):
        try:
            DamageExpr.parse(bad)
            raise AssertionError(f"expected parse error for {bad!r}")
        except ValueError:
            pass

    # melee_default: STR 10 -> STR/5 = 2; 1d6 -> [1,6]; range [3,8], mean 5.5.
    md = DamageExpr.melee_default()
    a10 = A.uniform(10)
    assert md.min_damage(a10) == 3 and md.max_damage(a10) == 8
    assert abs(md.mean_damage(a10) - 5.5) < 1e-9

    # Truncation toward zero on stat division.
    assert DamageExpr(stats=[StatTerm("strength", 1, 5)]).stat_total(A.uniform(12)) == 2
    print("damage_expr selftest OK")


if __name__ == "__main__":
    _selftest()
