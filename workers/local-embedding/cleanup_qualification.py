#!/usr/bin/env python3
"""Delete only this lane's explicit disposable qualification root."""

from __future__ import annotations

import argparse
import json
import shutil
from pathlib import Path


EXPECTED_LEAF = "embedding-qualification"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    if root.name.lower() != EXPECTED_LEAF or root.parent.name.lower() != "local-app-data":
        raise SystemExit("refusing cleanup outside the exact qualification root")
    existed = root.is_dir() and not root.is_symlink()
    if root.is_symlink():
        raise SystemExit("refusing cleanup of a symlink")
    if existed:
        shutil.rmtree(root)
    print(
        json.dumps(
            {
                "schema": "npc.embedding-qualification-cleanup/v1",
                "target_leaf": EXPECTED_LEAF,
                "existed_before": existed,
                "exists_after": root.exists(),
                "recoverable_pack_trash_removed": existed,
                "temporary_venv_wheelhouse_logs_and_pin_copies_removed": existed,
            },
            sort_keys=True,
        )
    )
    return 0 if not root.exists() else 4


if __name__ == "__main__":
    raise SystemExit(main())
