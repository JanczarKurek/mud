#!/usr/bin/env python3
"""Migrate a v8 mud2 world snapshot + accounts DB to v9.

v9 stops persisting the object registry and player object_ids; runtime ids
are reallocated on every load. The migration:

- Pulls each world object's per-instance `properties` out of the v8 registry
  section and folds them into the world_object dump.
- Drops the registry section entirely.
- Bumps `format_version` to 9.
- Removes the now-unused `object_id` / `combat_target_object_id` fields from
  every per-character `state_json` row in `accounts.db` (optional cleanup;
  serde ignores unknown fields anyway).

Run with no arguments for the embedded paths under
`~/.local/share/mud2/embedded/`, or pass `--save PATH --db PATH` to point at
specific files. `--dry-run` prints what would change without writing.
"""

import argparse
import json
import os
import shutil
import sqlite3
import sys
from pathlib import Path


def default_save() -> Path:
    return Path.home() / ".local/share/mud2/embedded/saves/world-state.json"


def default_db() -> Path:
    return Path.home() / ".local/share/mud2/embedded/accounts.db"


def migrate_world_snapshot(path: Path, dry_run: bool) -> None:
    if not path.exists():
        print(f"[snapshot] not found: {path} (skipping)")
        return
    with path.open() as f:
        dump = json.load(f)

    fv = dump.get("format_version")
    if fv == 9:
        print(f"[snapshot] {path}: already v9, nothing to do")
        return
    if fv != 8:
        print(f"[snapshot] {path}: format_version={fv} — only v8 → v9 supported, aborting")
        sys.exit(1)

    registry = dump.pop("object_registry", None)
    properties_by_id: dict[int, dict] = {}
    if registry and isinstance(registry, dict):
        for entry in registry.get("entries", []):
            properties_by_id[entry["object_id"]] = entry.get("properties") or {}

    world_objects = dump.get("world_objects", [])
    folded = 0
    for obj in world_objects:
        oid = obj.get("object_id")
        props = properties_by_id.get(oid, {})
        if "properties" not in obj:
            obj["properties"] = props
        if props:
            folded += 1

    dump["format_version"] = 9

    print(
        f"[snapshot] {path}: {len(world_objects)} world objects, "
        f"{folded} got properties from registry"
    )

    if dry_run:
        return

    backup = path.with_suffix(path.suffix + ".v8.bak")
    shutil.copy2(path, backup)
    print(f"[snapshot] backed up original to {backup}")
    with path.open("w") as f:
        json.dump(dump, f, indent=2)
        f.write("\n")
    print(f"[snapshot] wrote v9 to {path}")


def migrate_accounts_db(path: Path, dry_run: bool) -> None:
    if not path.exists():
        print(f"[db] not found: {path} (skipping)")
        return

    if not dry_run:
        backup = path.with_suffix(path.suffix + ".v8.bak")
        shutil.copy2(path, backup)
        print(f"[db] backed up original to {backup}")

    conn = sqlite3.connect(path)
    cur = conn.cursor()
    cur.execute("SELECT account_id, state_json FROM accounts WHERE state_json IS NOT NULL")
    rows = cur.fetchall()
    cleaned = 0
    for account_id, state_json in rows:
        if not state_json:
            continue
        try:
            data = json.loads(state_json)
        except json.JSONDecodeError as e:
            print(f"[db] account {account_id}: state_json failed to parse ({e}), skipping")
            continue
        changed = False
        for stale in ("object_id", "combat_target_object_id"):
            if stale in data:
                del data[stale]
                changed = True
        if changed:
            cleaned += 1
            if not dry_run:
                cur.execute(
                    "UPDATE accounts SET state_json = ?1, updated_at = strftime('%s','now') "
                    "WHERE account_id = ?2",
                    (json.dumps(data), account_id),
                )
    if not dry_run:
        conn.commit()
    print(f"[db] {path}: {len(rows)} character rows, {cleaned} cleaned")
    conn.close()


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--save", type=Path, default=default_save())
    p.add_argument("--db", type=Path, default=default_db())
    p.add_argument("--dry-run", action="store_true")
    args = p.parse_args()

    print(f"[plan] save = {args.save}")
    print(f"[plan] db   = {args.db}")
    print(f"[plan] dry-run = {args.dry_run}")
    migrate_world_snapshot(args.save, args.dry_run)
    migrate_accounts_db(args.db, args.dry_run)
    print("[done]")


if __name__ == "__main__":
    main()
