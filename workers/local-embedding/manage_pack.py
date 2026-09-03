#!/usr/bin/env python3
"""Explicit local-embedding pack lifecycle command surface.

The product will call the same PackLifecycle methods through the Rust Model
Manager.  This CLI exists for focused review and recovery; it never activates
the model and never performs inference.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from model_spec import PackSpec
from pack_manager import LifecycleError, PackLifecycle


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Manage the optional BGE embedding pack")
    parser.add_argument("action", choices=("install", "verify", "repair", "remove"))
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--install-root", type=Path, required=True)
    return parser.parse_args(argv)


def _report(report) -> dict[str, object]:  # type: ignore[no-untyped-def]
    return {
        "installed": report.installed,
        "healthy": report.healthy,
        "receipt_valid": report.receipt_valid,
        "checked_bytes": report.checked_bytes,
        "issues": [
            {
                "artifact_id": issue.artifact_id,
                "code": issue.code,
                "expected": issue.expected,
                "actual": issue.actual,
            }
            for issue in report.issues
        ],
    }


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        pack = PackSpec.load(args.manifest)
        lifecycle = PackLifecycle(args.install_root, pack)
        if args.action == "verify":
            result = {
                "schema": "npc.local-model-lifecycle-result/v1",
                "action": "verify",
                "target": str(lifecycle.target),
                "changed": False,
                "report": _report(lifecycle.verify()),
                "recovery_path": None,
            }
        else:
            operation = getattr(lifecycle, args.action)
            outcome = operation()
            result = {
                "schema": "npc.local-model-lifecycle-result/v1",
                "action": outcome.action,
                "target": str(outcome.target),
                "changed": outcome.changed,
                "report": _report(outcome.report),
                "recovery_path": str(outcome.recovery_path) if outcome.recovery_path else None,
            }
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0 if result["report"]["healthy"] or args.action == "remove" else 3  # type: ignore[index]
    except LifecycleError as exc:
        print(
            json.dumps(
                {
                    "schema": "npc.local-model-lifecycle-error/v1",
                    "code": exc.code,
                    "message": str(exc),
                    "details": exc.details,
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 4
    except (OSError, ValueError) as exc:
        print(
            json.dumps(
                {
                    "schema": "npc.local-model-lifecycle-error/v1",
                    "code": "invalid_configuration",
                    "message": str(exc),
                    "details": {},
                },
                indent=2,
                sort_keys=True,
            )
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

