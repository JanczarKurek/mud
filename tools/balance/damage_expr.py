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

from stats_model import AttributeSet, ability_mod


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
    # "raw" uses the full score; "mod" uses the d20 ability modifier
    # (`str_mod` at STR 16 -> +3), mirroring Rust's `StatTermMode`.
    mode: str = "raw"

    def value(self, attrs: AttributeSet) -> int:
        base = getattr(attrs, self.kind)
        if self.mode == "mod":
            base = ability_mod(base)
        raw = base * self.multiplier
        return _idiv(raw, self.divisor)


@dataclass(frozen=True)
class LevelTerm:
    """A ``level`` / ``lvl`` term, optionally scaled (``level/k`` or ``level*k``)."""
    multiplier: int = 1
    divisor: int = 1

    def value(self, level: int) -> int:
        raw = level * self.multiplier
        return _idiv(raw, self.divisor)


@dataclass
class DamageExpr:
    dice: Optional[Tuple[int, int]] = None     # (count, sides)
    stats: List[StatTerm] = field(default_factory=list)
    bonus: int = 0
    level: List[LevelTerm] = field(default_factory=list)

    # --- construction -----------------------------------------------------
    @staticmethod
    def melee_default() -> "DamageExpr":
        """Default unarmed/no-`damage:` fallback: ``1d4 + str_mod`` (the floor)."""
        return DamageExpr(dice=(1, 4), stats=[StatTerm("strength", 1, 1, "mod")], bonus=0)

    @staticmethod
    def parse(raw: str) -> "DamageExpr":
        trimmed = raw.strip()
        if not trimmed:
            raise ValueError("empty damage expression")

        dice: Optional[Tuple[int, int]] = None
        stats: List[StatTerm] = []
        bonus = 0
        level: List[LevelTerm] = []

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

            # Attribute or level term, optionally scaled.
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

            stat_lower = stat_part.strip().lower()
            if stat_lower in ("level", "lvl"):
                level.append(LevelTerm(multiplier, divisor))
                continue

            # A `_mod` suffix (e.g. `str_mod`, `focus_mod`) switches the term
            # to ability-modifier mode.
            mode = "raw"
            if stat_lower.endswith("_mod"):
                stat_lower = stat_lower[: -len("_mod")]
                mode = "mod"
            key = _ATTR_ALIASES.get(stat_lower)
            if key is None:
                raise ValueError(f"unrecognized term '{term}' in '{raw}'")
            stats.append(StatTerm(key, multiplier, divisor, mode))

        return DamageExpr(dice=dice, stats=stats, bonus=bonus, level=level)

    # --- evaluation -------------------------------------------------------
    def stat_total(self, attrs: AttributeSet) -> int:
        return sum(term.value(attrs) for term in self.stats)

    def level_total(self, level: int) -> int:
        return sum(term.value(level) for term in self.level)

    def roll(self, attrs: AttributeSet, rng, level: int = 0) -> int:
        dice_total = 0
        if self.dice:
            count, sides = self.dice
            dice_total = sum(rng.randint(1, sides) for _ in range(count))
        return dice_total + self.stat_total(attrs) + self.bonus + self.level_total(level)

    def min_damage(self, attrs: AttributeSet, level: int = 0) -> int:
        dice_total = self.dice[0] if self.dice else 0          # every die shows 1
        return dice_total + self.stat_total(attrs) + self.bonus + self.level_total(level)

    def max_damage(self, attrs: AttributeSet, level: int = 0) -> int:
        dice_total = self.dice[0] * self.dice[1] if self.dice else 0
        return dice_total + self.stat_total(attrs) + self.bonus + self.level_total(level)

    def mean_damage(self, attrs: AttributeSet, level: int = 0) -> float:
        """Expected roll (no min-1 floor; the floor lives in the combat model)."""
        dice_mean = 0.0
        if self.dice:
            count, sides = self.dice
            dice_mean = count * (sides + 1) / 2.0
        return dice_mean + self.stat_total(attrs) + self.bonus + self.level_total(level)

    def describe(self) -> str:
        parts = []
        if self.dice:
            parts.append(f"{self.dice[0]}d{self.dice[1]}")
        for t in self.stats:
            s = t.kind[:3].upper()
            if t.mode == "mod":
                s += "mod"
            if t.multiplier != 1:
                s += f"*{t.multiplier}"
            if t.divisor != 1:
                s += f"/{t.divisor}"
            parts.append(s)
        for t in self.level:
            s = "LVL"
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

    # melee_default (unarmed floor): STR 10 -> mod +0; 1d4 -> [1,4], mean 2.5.
    md = DamageExpr.melee_default()
    a10 = A.uniform(10)
    assert md.min_damage(a10) == 1 and md.max_damage(a10) == 4
    assert abs(md.mean_damage(a10) - 2.5) < 1e-9
    assert md == DamageExpr.parse("1d4+str_mod")

    # Truncation toward zero on stat division.
    assert DamageExpr(stats=[StatTerm("strength", 1, 5)]).stat_total(A.uniform(12)) == 2

    # Level term: "1d8+focus/2+level/2" at FOC 18, level 10 -> min = 1+9+5 = 15.
    lvl = DamageExpr.parse("1d8+focus/2+level/2")
    assert lvl.level and lvl.level[0].divisor == 2
    foc18 = A(focus=18)
    assert lvl.level_total(10) == 5
    assert lvl.min_damage(foc18, level=10) == 15, lvl.min_damage(foc18, level=10)
    # No level passed -> level term contributes nothing (existing weapon callers).
    assert lvl.min_damage(foc18) == 1 + 9

    # `_mod` terms mirror Rust's StatTermMode::Mod (anchors from damage_expr.rs).
    mod_e = DamageExpr.parse("1d8+str_mod")
    assert mod_e.stats[0].mode == "mod"
    a16 = A(strength=16)
    assert mod_e.min_damage(a16) == 1 + 3 and mod_e.max_damage(a16) == 8 + 3
    long_mod = DamageExpr.parse("focus_mod*2")
    assert long_mod.stat_total(A(focus=14)) == 4
    # STR 7 -> mod -2 (rounded toward -inf).
    neg = DamageExpr.parse("str_mod+5")
    assert neg.min_damage(A(strength=7)) == -2 + 5
    # Raw terms keep raw-score meaning.
    assert hp.stats[0].mode == "raw"
    print("damage_expr selftest OK")


if __name__ == "__main__":
    _selftest()
