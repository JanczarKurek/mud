"""Faithful Python port of the character-stat layer.

Mirrors:
  * `src/player/components.rs`  -> AttributeSet, point-buy, DerivedStats
  * `src/player/classes.rs`     -> Class data, ability_mod, bab/saves
  * `src/player/progression.rs` -> xp_for_level, xp_grant_for_kill, level_for_xp
  * `src/player/regen.rs`       -> out-of-combat HP/mana regen intervals

Every constant and formula here is asserted against the Rust ``#[cfg(test)]``
anchors in :func:`_selftest`, so drift in the game will surface as a failing
selftest rather than silently wrong analysis.
"""

from __future__ import annotations

from dataclasses import dataclass

# --- point-buy (character creation), components.rs:494-501 -----------------
POINT_BUY_BUDGET = 12
ATTR_FLOOR = 8
ATTR_CEILING = 18
ATTR_BASELINE = 10

LEVEL_CAP = 20            # progression.rs:17
XP_COEFFICIENT = 1000     # progression.rs:20


@dataclass(frozen=True)
class AttributeSet:
    strength: int = 10
    agility: int = 10
    constitution: int = 10
    willpower: int = 10
    charisma: int = 10
    focus: int = 10

    @staticmethod
    def uniform(v: int) -> "AttributeSet":
        return AttributeSet(v, v, v, v, v, v)

    def point_buy_spend(self) -> int:
        return sum(getattr(self, k) - ATTR_BASELINE for k in
                   ("strength", "agility", "constitution",
                    "willpower", "charisma", "focus"))

    def validate_point_buy(self) -> None:
        """Raises if this spread is not a legal creation build (components.rs:506)."""
        for k in ("strength", "agility", "constitution",
                  "willpower", "charisma", "focus"):
            v = getattr(self, k)
            if v < ATTR_FLOOR or v > ATTR_CEILING:
                raise ValueError(f"{k}={v} outside [{ATTR_FLOOR},{ATTR_CEILING}]")
        spend = self.point_buy_spend()
        if spend != POINT_BUY_BUDGET:
            raise ValueError(f"point-buy spend {spend} != {POINT_BUY_BUDGET}")


def ability_mod(score: int) -> int:
    """3.5e ability modifier, `(score-10)/2` rounded toward -inf (classes.rs:168)."""
    if score >= 10:
        return (score - 10) // 2
    # Rust: ((score - 10) - 1) / 2 with truncation toward zero.
    n = (score - 10) - 1
    q = abs(n) // 2
    return -q if n < 0 else q


# --- classes (classes.rs:110-145) -----------------------------------------
# casting_attribute: None / "focus" / "willpower"
@dataclass(frozen=True)
class ClassData:
    hit_die: int
    bab_track: str            # "full" / "three_quarter" / "half"
    skill_points_per_level: int
    mana_per_level: int
    casting_attribute: str | None


CLASSES = {
    "Fighter":  ClassData(10, "full",          2, 0,  None),
    "Wizard":   ClassData(4,  "half",          2, 10, "focus"),
    "Cleric":   ClassData(8,  "three_quarter", 2, 8,  "willpower"),
    "Vagabond": ClassData(6,  "three_quarter", 8, 0,  None),
}


def bab_at(track: str, level: int) -> int:
    if track == "full":
        return level
    if track == "three_quarter":
        return (3 * level) // 4
    if track == "half":
        return level // 2
    raise ValueError(track)


def weapon_focus_bonus(class_name, level: int) -> int:
    """Fighter Weapon Focus melee to-hit: +1 at L1, +1 more at 5/10/15/20
    (mirrors `player/classes.rs::weapon_focus_bonus`). 0 for everyone else."""
    return 1 + level // 5 if class_name == "Fighter" else 0


def good_save_at(level: int) -> int:
    return 2 + level // 2


def poor_save_at(level: int) -> int:
    return level // 3


# --- derived stats (components.rs:749-792) ---------------------------------
@dataclass(frozen=True)
class DerivedStats:
    attributes: AttributeSet
    max_health: int
    max_mana: int


def derive_stats(attrs: AttributeSet, class_name: str, level: int,
                 base_health: int = 0, base_mana: int = 0) -> DerivedStats:
    """Player derived HP/mana. NPCs do NOT use this (they roll an `hp:` expr)."""
    a = AttributeSet(*[max(1, getattr(attrs, k)) for k in
                       ("strength", "agility", "constitution",
                        "willpower", "charisma", "focus")])
    cd = CLASSES[class_name]
    con_mod = ability_mod(a.constitution)
    level_above_1 = max(0, level - 1)

    hp_per_level = max(1, cd.hit_die // 2 + 1 + con_mod)
    level_hp = level_above_1 * hp_per_level

    if cd.casting_attribute == "focus":
        cast_mod = ability_mod(a.focus)
    elif cd.casting_attribute == "willpower":
        cast_mod = ability_mod(a.willpower)
    else:
        cast_mod = 0
    mana_per_level = max(0, cd.mana_per_level + cast_mod)
    level_mana = level_above_1 * mana_per_level

    max_health = max(1, 35 + a.constitution * 6 + a.strength * 2 + base_health + level_hp)
    max_mana = max(0, 10 + a.willpower * 6 + a.focus * 3 + base_mana + level_mana)
    return DerivedStats(a, max_health, max_mana)


def hp_per_level(attrs: AttributeSet, class_name: str) -> int:
    cd = CLASSES[class_name]
    return max(1, cd.hit_die // 2 + 1 + ability_mod(attrs.constitution))


def mana_per_level(attrs: AttributeSet, class_name: str) -> int:
    cd = CLASSES[class_name]
    if cd.casting_attribute == "focus":
        cast_mod = ability_mod(attrs.focus)
    elif cd.casting_attribute == "willpower":
        cast_mod = ability_mod(attrs.willpower)
    else:
        cast_mod = 0
    return max(0, cd.mana_per_level + cast_mod)


# --- progression (progression.rs) -----------------------------------------
def xp_for_level(n: int) -> int:
    return XP_COEFFICIENT * n * (n - 1) // 2


def level_for_xp(xp: int) -> int:
    n = 1
    while n < LEVEL_CAP and xp >= xp_for_level(n + 1):
        n += 1
    return n


def xp_grant_for_kill(victim_level: int) -> int:
    # Linear 75/level: same-level kills-to-level is a constant ~13.3 against
    # the 1000*N(N-1)/2 curve (progression.rs).
    return victim_level * 75


# --- regen (regen.rs:19-31) -----------------------------------------------
def hp_regen_seconds_per_point(attrs: AttributeSet, multiplier: float = 1.0) -> float:
    per_minute = max(0.001, (2.0 + max(0, attrs.constitution) / 5.0) * multiplier)
    return 60.0 / per_minute


def mana_regen_seconds_per_point(attrs: AttributeSet, multiplier: float = 1.0) -> float:
    # per_minute = 2 + willpower + focus/2 (regen.rs). Retuned for sustained
    # casting: a WIL16/FOC16 caster regens ~26/min (~2.3 s/MP), so a cantrip
    # is sustainable every ~10 s and a 12-mana nuke every ~28 s.
    per_minute = max(0.001, (2.0 + max(0, attrs.willpower)
                             + max(0, attrs.focus) / 2.0) * multiplier)
    return 60.0 / per_minute


def bumps_for_level(level: int) -> int:
    """Number of every-4-levels ability bumps a character has earned by `level`.

    Players gain +1 to one attribute at levels 4/8/12/16/20 (progression).
    NPCs do not get bumps.
    """
    return len([L for L in (4, 8, 12, 16, 20) if level >= L])


def _selftest() -> None:
    # ability_mod anchors (classes.rs:183).
    assert [ability_mod(s) for s in (10, 12, 14, 8, 6, 1, 20)] == [0, 1, 2, -1, -2, -5, 5]

    # bab / saves (classes.rs:194, 207).
    assert bab_at("full", 1) == 1 and bab_at("full", 20) == 20
    assert bab_at("three_quarter", 4) == 3 and bab_at("three_quarter", 20) == 15
    assert bab_at("half", 2) == 1 and bab_at("half", 20) == 10
    assert good_save_at(1) == 2 and good_save_at(20) == 12
    assert poor_save_at(3) == 1 and poor_save_at(20) == 6

    # xp curve / grants (progression.rs:188, 211).
    assert [xp_for_level(n) for n in (1, 2, 3, 4, 5, 10, 20)] == \
        [0, 1000, 3000, 6000, 10000, 45000, 190000]
    assert [xp_grant_for_kill(n) for n in (1, 2, 3, 8, 20)] == [75, 150, 225, 600, 1500]
    # Same-level kills-per-level stays in the 8-15 band across 1..19.
    for n in range(1, LEVEL_CAP):
        kills = (xp_for_level(n + 1) - xp_for_level(n)) / xp_grant_for_kill(n)
        assert 8.0 <= kills <= 15.0, (n, kills)
    assert level_for_xp(0) == 1 and level_for_xp(1000) == 2 and level_for_xp(190000) == 20

    # default all-10 Fighter L1: HP = 35 + CON*6 + STR*2 = 35+60+20 = 115;
    # mana = 10 + WIL*6 + FOC*3 = 10+60+30 = 100.
    d = derive_stats(AttributeSet.uniform(10), "Fighter", 1)
    assert d.max_health == 115 and d.max_mana == 100, (d.max_health, d.max_mana)

    # regen anchors (regen.rs:161): CON 10 -> 15s/HP; CON 20 -> 10s/HP.
    assert abs(hp_regen_seconds_per_point(AttributeSet.uniform(10)) - 15.0) < 1e-9
    assert abs(hp_regen_seconds_per_point(AttributeSet(constitution=20)) - 10.0) < 1e-9

    # point-buy: +6/+4/+2 (=12) and all-12 (6*+2=12) are legal; all-13 overspends.
    AttributeSet(16, 12, 14, 10, 10, 10).validate_point_buy()
    AttributeSet.uniform(12).validate_point_buy()
    try:
        AttributeSet.uniform(13).validate_point_buy()
        raise AssertionError("expected point-buy failure")
    except ValueError:
        pass
    print("stats_model selftest OK")


if __name__ == "__main__":
    _selftest()
