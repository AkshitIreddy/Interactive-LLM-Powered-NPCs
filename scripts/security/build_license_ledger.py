#!/usr/bin/env python3
"""Offline maintainer tool for refreshing the committed lock/license ledger.

It never installs or fetches packages. Missing metadata is a hard error unless
the package has an explicitly reviewed override below.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import tomllib
from pathlib import Path

from lock_inventory import load_inventory


REVIEWED_OVERRIDES = {
    "pkg:cargo/arbitrary@1.4.2": "MIT OR Apache-2.0",
    "pkg:cargo/derive_arbitrary@1.4.2": "MIT OR Apache-2.0",
    "pkg:cargo/fiat-crypto@0.2.9": "MIT OR Apache-2.0",
    "pkg:cargo/signal-hook-registry@1.4.8": "Apache-2.0 OR MIT",
    "pkg:cargo/wasm-streams@0.4.2": "MIT",
    "pkg:cargo/web-time@1.1.0": "MIT OR Apache-2.0",
    "pkg:cargo/windows-sys@0.52.0": "MIT OR Apache-2.0",
    "pkg:cargo/xattr@1.6.1": "MIT OR Apache-2.0",
    "pkg:npm/@babel/plugin-transform-react-jsx-self@7.29.7": "MIT",
    "pkg:npm/@babel/plugin-transform-react-jsx-source@7.29.7": "MIT",
    "pkg:npm/@csstools/css-color-parser@3.1.0": "MIT",
    "pkg:npm/@csstools/css-parser-algorithms@3.0.5": "MIT",
    "pkg:npm/@napi-rs/lzma-linux-x64-gnu@1.5.1": "MIT",
    "pkg:npm/@react-aria/visually-hidden@3.9.1": "Apache-2.0",
    "pkg:npm/@testing-library/jest-dom@7.0.1": "MIT",
    "pkg:npm/@testing-library/react@16.3.3": "MIT",
    "pkg:npm/@testing-library/user-event@14.6.6": "MIT",
    "pkg:npm/fsevents@2.3.3": "MIT",
    "pkg:npm/parse-cache-control@1.0.1": "MIT",
    "pkg:npm/playwright-core@1.62.1": "Apache-2.0",
    "pkg:npm/react-aria-components@1.20.0": "Apache-2.0",
}
NORMALIZED_EXPRESSIONS = {
    "Apache-2.0 / MIT": "Apache-2.0 OR MIT",
    "Apache-2.0/MIT": "Apache-2.0 OR MIT",
    "BSD-3-Clause/MIT": "BSD-3-Clause OR MIT",
    "MIT/Apache-2.0": "MIT OR Apache-2.0",
    "Unlicense/MIT": "Unlicense OR MIT",
}


def override_for(purl: str) -> str | None:
    if purl in REVIEWED_OVERRIDES:
        return REVIEWED_OVERRIDES[purl]
    if purl.startswith("pkg:cargo/interactive-npcs-") or purl.startswith("pkg:cargo/npc-") or purl.startswith("pkg:cargo/model-manager@"):
        return "MIT"
    if purl.startswith("pkg:npm/@esbuild/"):
        return "MIT"
    if purl.startswith("pkg:npm/@rollup/rollup-"):
        return "MIT"
    if purl.startswith("pkg:npm/@tauri-apps/cli-"):
        return "Apache-2.0 OR MIT"
    return None


def workspace_cargo_metadata(root: Path) -> dict[tuple[str, str], str]:
    result: dict[tuple[str, str], str] = {}
    paths = subprocess.run(
        ["git", "-C", str(root), "ls-files", "--cached", "--others", "--exclude-standard", "*Cargo.toml", "**/Cargo.toml"],
        capture_output=True, text=True, check=True,
    ).stdout.splitlines()
    for relative in sorted(set(paths)):
        try:
            package = tomllib.loads((root / relative).read_text(encoding="utf-8")).get("package", {})
            if package.get("name") and package.get("version") and package.get("license"):
                result[(str(package["name"]), str(package["version"]))] = str(package["license"])
        except (OSError, tomllib.TOMLDecodeError):
            continue
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--cargo-registry", type=Path, default=Path.home() / ".cargo/registry/src")
    args = parser.parse_args()
    root = args.root.resolve()
    components, sources = load_inventory(root)
    cargo_dirs: dict[str, Path] = {}
    if args.cargo_registry.is_dir():
        for registry in args.cargo_registry.iterdir():
            if registry.is_dir():
                cargo_dirs.update({entry.name: entry for entry in registry.iterdir() if entry.is_dir()})
    pnpm_root = root / "node_modules/.pnpm"
    pnpm_dirs = [entry.name for entry in pnpm_root.iterdir() if entry.is_dir()] if pnpm_root.is_dir() else []
    workspace = workspace_cargo_metadata(root)
    licenses: dict[str, str] = {}
    missing: list[str] = []
    for component in components:
        purl, name, version = component["purl"], component["name"], component["version"]
        license_expression = override_for(purl)
        if purl.startswith("pkg:cargo/"):
            license_expression = license_expression or workspace.get((name, version))
            package_dir = cargo_dirs.get(f"{name}-{version}")
            if not license_expression and package_dir:
                try:
                    license_expression = tomllib.loads((package_dir / "Cargo.toml").read_text(encoding="utf-8")).get("package", {}).get("license")
                except (OSError, tomllib.TOMLDecodeError):
                    pass
        else:
            encoded_name = name.replace("/", "+")
            prefix = f"{encoded_name}@{version}"
            folder = next((item for item in pnpm_dirs if item == prefix or item.startswith(prefix + "_")), None)
            candidates = []
            if folder:
                candidates.append(pnpm_root / folder / "node_modules" / name / "package.json")
            candidates.append(root / "demo/readme/node_modules" / name / "package.json")
            for metadata_path in candidates:
                if license_expression or not metadata_path.is_file():
                    continue
                try:
                    value = json.loads(metadata_path.read_text(encoding="utf-8")).get("license")
                    license_expression = value.get("type") if isinstance(value, dict) else value
                except (OSError, json.JSONDecodeError):
                    continue
        if isinstance(license_expression, str) and license_expression.strip():
            cleaned = license_expression.strip()
            licenses[purl] = NORMALIZED_EXPRESSIONS.get(cleaned, cleaned)
        else:
            missing.append(purl)
    if missing:
        raise RuntimeError("license metadata unavailable for:\n" + "\n".join(missing))
    document = {
        "schema_version": 1,
        "generator": "scripts/security/build_license_ledger.py@1",
        "source_locks": sources,
        "components": dict(sorted(licenses.items())),
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.out), "components": len(licenses)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
