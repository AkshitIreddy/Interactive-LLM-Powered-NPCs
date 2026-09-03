#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from npc_local_stt.pack_lifecycle import PackError, PackLifecycle, PackManifest


def main() -> int:
    parser = argparse.ArgumentParser(description="Explicit lifecycle for the optional local STT pack")
    parser.add_argument("operation", choices=["inspect", "install", "verify", "repair", "remove"])
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--root", type=Path, help="dedicated model-manager pack root")
    parser.add_argument("--confirm-license-and-download", choices=["YES"], default=None)
    args = parser.parse_args()
    try:
        manifest = PackManifest.load(args.manifest)
        if args.root is None:
            if args.operation != "inspect":
                raise PackError("root_required", "--root is required for lifecycle operations")
            root = Path.cwd() / ".npc-pack-inspect-only"
        else:
            root = args.root
        lifecycle = PackLifecycle(manifest, root)
        if args.operation == "inspect":
            result = lifecycle.inspect()
        elif args.operation == "install":
            result = lifecycle.install(confirmed=args.confirm_license_and_download == "YES")
        elif args.operation == "verify":
            result = lifecycle.verify()
        elif args.operation == "repair":
            result = lifecycle.repair(confirmed=args.confirm_license_and_download == "YES")
        else:
            result = lifecycle.remove()
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except PackError as error:
        print(json.dumps({"ok": False, "error": {"code": error.code, "message": error.safe_message}}))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
