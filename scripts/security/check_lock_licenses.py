#!/usr/bin/env python3
"""Offline, fail-closed license and provenance validation for committed locks."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import tomllib
from pathlib import Path

from lock_inventory import InventoryError, load_inventory


LICENSE_TOKEN = re.compile(r"[A-Za-z0-9][A-Za-z0-9.-]*(?:\+)?")
OPERATORS = {"AND", "OR", "WITH"}


class ExpressionError(ValueError):
    pass


def expression_is_allowed(expression: str, allowed: set[str], restricted: set[str]) -> tuple[bool, set[str]]:
    """Evaluate the license choices in the small SPDX subset used by the ledger.

    OR means the distributor may select an allowed branch. AND and WITH require
    every term. Unknown identifiers remain fail closed unless an alternate OR
    branch is independently allowed.
    """
    tokens = re.findall(r"\(|\)|AND|OR|WITH|[A-Za-z0-9][A-Za-z0-9.+-]*", expression)
    unknown = {token for token in tokens if token not in OPERATORS and token not in {"(", ")"} and token not in allowed and token not in restricted}
    position = 0

    def primary() -> bool:
        nonlocal position
        if position >= len(tokens):
            raise ExpressionError("unexpected end")
        token = tokens[position]
        if token == "(":
            position += 1
            value = disjunction()
            if position >= len(tokens) or tokens[position] != ")":
                raise ExpressionError("missing closing parenthesis")
            position += 1
            return value
        if token in OPERATORS or token == ")":
            raise ExpressionError(f"unexpected {token}")
        position += 1
        return token in allowed

    def conjunction() -> bool:
        nonlocal position
        value = primary()
        while position < len(tokens) and tokens[position] in {"AND", "WITH"}:
            position += 1
            value = primary() and value
        return value

    def disjunction() -> bool:
        nonlocal position
        value = conjunction()
        while position < len(tokens) and tokens[position] == "OR":
            position += 1
            value = conjunction() or value
        return value

    value = disjunction()
    if position != len(tokens):
        raise ExpressionError(f"unexpected {tokens[position]}")
    return value, unknown


def model_provenance_error(manifest: dict) -> str | None:
    license_value = manifest.get("license") or manifest.get("licenses")
    if not license_value:
        return "missing license provenance"
    if isinstance(license_value, dict):
        has_license_id = bool(
            license_value.get("spdx_expression")
            or license_value.get("spdx")
            or license_value.get("entries")
        )
        if not has_license_id:
            return "license provenance has no SPDX expression or entries"
    source_ok = bool(
        (manifest.get("source_project") and manifest.get("source_revision"))
        or manifest.get("source")
        or manifest.get("upstream")
        or manifest.get("provenance")
        or (isinstance(manifest.get("model"), dict) and manifest["model"].get("upstream_model_commit"))
    )
    if not source_ok:
        return "missing immutable source provenance"
    return None


def is_model_pack_manifest(document: dict) -> bool:
    """Separate installable pack manifests from catalogs and review evidence."""
    return document.get("schema") in {"npc.model-pack/v1", "npc.model-pack/v2"}


def validate_distribution_ledger(root: Path) -> tuple[int, int]:
    path = root / "packaging/security/distribution-components.json"
    try:
        ledger = json.loads(path.read_text(encoding="utf-8"))
        components = ledger["components"]
        static_files = ledger["static_files"]
        static_rules = ledger["static_inventory_rules"]
        profiles = ledger["distribution_profiles"]
    except (OSError, KeyError, json.JSONDecodeError) as exc:
        raise ExpressionError(f"cannot read distribution ledger: {exc}") from exc
    if ledger.get("schema_version") != 1 or not isinstance(components, dict) or not isinstance(static_files, list):
        raise ExpressionError("invalid distribution ledger schema")
    required_component_fields = {"name", "spdx", "distribution_class", "redistributed", "scopes", "hash_policy", "source_reference", "notice_reference"}
    for component_id, entry in components.items():
        missing = sorted(required_component_fields - set(entry))
        if missing:
            raise ExpressionError(f"distribution component {component_id} is missing {missing}")
    seen_paths: set[str] = set()
    for entry in static_files:
        source = entry.get("path")
        if not isinstance(source, str) or source in seen_paths:
            raise ExpressionError(f"invalid or duplicate static distribution path: {source!r}")
        seen_paths.add(source)
        if entry.get("component_id") not in components:
            raise ExpressionError(f"static distribution path has unknown component: {source}")
        source_path = root / source
        if not source_path.is_file():
            raise ExpressionError(f"static distribution path is missing: {source}")
        digest = hashlib.sha256(source_path.read_bytes()).hexdigest()
        if digest != entry.get("sha256"):
            raise ExpressionError(f"static distribution hash is stale: {source} (expected {entry.get('sha256')}, got {digest})")
    protected_paths: set[str] = set()
    for rule in static_rules:
        rule_root = root / rule["root"]
        if not rule_root.is_dir():
            raise ExpressionError(f"static inventory root is missing: {rule['root']}")
        protected_paths.update(item.relative_to(root).as_posix() for item in rule_root.glob(rule["glob"]) if item.is_file())
    unexplained_protected = sorted(protected_paths - seen_paths)
    stale_protected = sorted(path for path in seen_paths - protected_paths if any(path == rule["root"] or path.startswith(rule["root"] + "/") for rule in static_rules))
    if unexplained_protected or stale_protected:
        raise ExpressionError(f"static inventory differs from protected roots (unexplained={unexplained_protected}, stale={stale_protected})")
    for profile_name in ("installer", "test-game"):
        if profile_name not in profiles:
            raise ExpressionError(f"missing distribution profile: {profile_name}")
    font_binary_suffixes = {".ttf", ".otf", ".woff", ".woff2", ".ttc", ".eot"}
    bundled_fonts = [item.relative_to(root).as_posix() for item in (root / "assets/subtitles").rglob("*") if item.is_file() and item.suffix.lower() in font_binary_suffixes]
    if bundled_fonts:
        raise ExpressionError(f"subtitle font policy says no binaries, but found: {bundled_fonts}")
    return len(components), len(static_files)


def validate_license_material_overrides(root: Path, declared: dict[str, str]) -> int:
    path = root / "packaging/security/license-material-overrides.json"
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
        components = document["components"]
        license_files = document["license_files"]
    except (OSError, KeyError, json.JSONDecodeError) as exc:
        raise ExpressionError(f"cannot read license-material overrides: {exc}") from exc
    if document.get("schema_version") != 1 or not components:
        raise ExpressionError("license-material overrides must be schema v1 and non-empty")
    for purl, component in components.items():
        if purl not in declared or component.get("declared_spdx") != declared[purl]:
            raise ExpressionError(f"license-material override does not match lock ledger: {purl}")
        crate_revision = str(component.get("crate_source_revision", ""))
        if not re.fullmatch(r"[a-f0-9]{40}", crate_revision):
            raise ExpressionError(f"license-material override lacks pinned crate revision: {purl}")
        for hash_field in ("crate_vcs_info_sha256", "cargo_toml_orig_sha256"):
            if not re.fullmatch(r"[a-f0-9]{64}", str(component.get(hash_field, ""))):
                raise ExpressionError(f"license-material override lacks {hash_field}: {purl}")
        file_ids = component.get("license_file_ids")
        if not isinstance(file_ids, list):
            file_ids = [component.get("license_file_id")]
        if not file_ids or any(file_id not in license_files for file_id in file_ids):
            raise ExpressionError(f"license-material override has unknown license file: {purl}")
        for file_id in file_ids:
            license_file = license_files[file_id]
            source_kind = license_file.get("source_kind", "pinned-vcs-blob")
            source_revision = str(license_file.get("source_revision", ""))
            if source_kind == "pinned-vcs-blob":
                if not re.fullmatch(r"[a-f0-9]{40}", source_revision) or source_revision not in str(license_file.get("source_url", "")):
                    raise ExpressionError(f"license-material override is not pinned to its source revision: {purl}")
            elif source_kind == "official-standard-license-text":
                if license_file.get("standard_id") != component.get("declared_spdx") or not str(license_file.get("source_url", "")).startswith("https://"):
                    raise ExpressionError(f"official standard license body does not match declaration: {purl}")
            else:
                raise ExpressionError(f"unknown license-material source kind: {purl}")
            vendored_path = root / str(license_file.get("path", ""))
            if not vendored_path.is_file():
                raise ExpressionError(f"vendored license-material override is missing: {purl}")
            data = vendored_path.read_bytes()
            if hashlib.sha256(data).hexdigest() != license_file.get("sha256"):
                raise ExpressionError(f"vendored license-material override hash mismatch: {purl}")
            upstream_sha = license_file.get("upstream_sha256")
            if license_file.get("normalization"):
                rewrites = license_file.get("markdown_link_rewrites")
                if rewrites:
                    reconstructed = data
                    for rewrite in rewrites:
                        upstream = f"]({rewrite.get('upstream', '')})".encode()
                        vendored = f"]({rewrite.get('vendored', '')})".encode()
                        if not rewrite.get("upstream") or not rewrite.get("vendored") or reconstructed.count(vendored) != 1:
                            raise ExpressionError(f"license-material Markdown rewrite is not exact: {purl}")
                        reconstructed = reconstructed.replace(vendored, upstream)
                    if hashlib.sha256(reconstructed).hexdigest() != upstream_sha:
                        raise ExpressionError(f"license-material Markdown rewrites do not reproduce upstream bytes: {purl}")
                elif not data.endswith(b"\n") or hashlib.sha256(data[:-1]).hexdigest() != upstream_sha:
                    raise ExpressionError(f"license-material override normalization does not reproduce upstream bytes: {purl}")
    return len(components)


def validate_installer_toolchain_provenance(root: Path) -> int:
    path = root / "packaging/security/installer-toolchain-provenance.json"
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
        tauri = document["tauri_cli"]
        nsis = document["nsis_3_11"]
        utils = document["nsis_tauri_utils_0_5_3"]
        webview = document["webview2_evergreen_standalone_x64"]
    except (OSError, KeyError, json.JSONDecodeError) as exc:
        raise ExpressionError(f"cannot read installer toolchain provenance: {exc}") from exc
    if document.get("schema_version") != 1:
        raise ExpressionError("installer toolchain provenance must be schema v1")

    expected_pins = {
        "tauri-version": (tauri.get("version"), "2.11.4"),
        "tauri-native-sha256": (tauri.get("windows_native_sha256"), "37c4d79256120893f2c12c1385bce5f7510b5063afdeb09b40a9effec28d0208"),
        "tauri-nsis-source-sha256": (tauri.get("nsis_bundler_source", {}).get("sha256"), "b1c6db4e8a9bcc55ed6a739fe73a64a18801600bbefc76083f7449db4fb87a0f"),
        "tauri-windows-source-sha256": (tauri.get("windows_utility_source", {}).get("sha256"), "2a3be006fee604cb75ea6d847d561a32e31d82006e2e1ddf3c476fad3e2028fd"),
        "nsis-binary-sha1": (nsis.get("binary_archive", {}).get("tauri_embedded_sha1"), "ef7ff767e5cbd9edd22add3a32c9b8f4500bb10d"),
        "nsis-binary-sha256": (nsis.get("binary_archive", {}).get("reviewed_sha256"), "c7d27f780ddb6cffb4730138cd1591e841f4b7edb155856901cdf5f214394fa1"),
        "nsis-source-sha256": (nsis.get("corresponding_source_archive", {}).get("sha256"), "19e72062676ebdc67c11dc032ba80b979cdbffd3886c60b04bb442cdd401ff4b"),
        "nsis-utils-commit": (utils.get("source", {}).get("commit"), "13d9edd27b69310e108d6fbd49f90992f8a05390"),
        "nsis-utils-binary-sha1": (utils.get("binary", {}).get("tauri_embedded_sha1"), "75197fee3c6a814fe035788d1c34ead39349b860"),
        "nsis-utils-binary-sha256": (utils.get("binary", {}).get("reviewed_sha256"), "5ba143b5db4a87d32d6e7802e033330aae56cbceabe0d1e3ba41948385ad4709"),
        "webview-mode": (webview.get("configured_mode"), "skip"),
        "webview-contract": (webview.get("packaging_contract"), "customPinnedOfflineInstaller"),
        "webview-cache-path": (webview.get("canonical_cache_path"), "%LOCALAPPDATA%\\InteractiveNPCs\\toolchain\\webview2\\1.3.263.3\\987a9d8b3107e84f9b53b4a077d28ae4814fc3d964d5a55c559e7334bbf24d61\\MicrosoftEdgeWebView2RuntimeInstallerX64.exe"),
        "webview-url": (webview.get("tauri_url"), "https://go.microsoft.com/fwlink/?linkid=2124701"),
        "webview-resolved-url": (webview.get("resolved_url"), "https://msedge.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/b7e683e6-e94c-4576-bfe5-34852785a4d6/MicrosoftEdgeWebView2RuntimeInstallerX64.exe"),
        "webview-sha256": (webview.get("sha256"), "987a9d8b3107e84f9b53b4a077d28ae4814fc3d964d5a55c559e7334bbf24d61"),
        "webview-version": (webview.get("file_version"), "1.3.263.3"),
        "webview-signer": (webview.get("authenticode", {}).get("signer_thumbprint"), "4028CAD637509D4744B17EC5B42AED8D7A31E6AF"),
    }
    mismatches = [name for name, (actual, expected) in expected_pins.items() if actual != expected]
    if mismatches:
        raise ExpressionError(f"installer toolchain reviewed pins changed: {mismatches}")

    lock_text = (root / "pnpm-lock.yaml").read_text(encoding="utf-8")
    for field in ("npm_integrity", "windows_native_integrity"):
        if str(tauri.get(field, "")).removeprefix("sha512-") not in lock_text:
            raise ExpressionError(f"installer toolchain {field} is not present in pnpm-lock.yaml")
    configs = [
        root / "apps/control/src-tauri/tauri.conf.json",
        root / "packaging/windows/tauri.release.conf.json",
        root / "packaging/windows/tauri.review.conf.json",
    ]
    for config_path in configs:
        config = json.loads(config_path.read_text(encoding="utf-8"))
        mode = config.get("bundle", {}).get("windows", {}).get("webviewInstallMode", {}).get("type")
        if mode != webview["configured_mode"]:
            raise ExpressionError(f"WebView2 mode differs from installer provenance: {config_path.relative_to(root)}")
    for license_entry in [nsis["license"], *utils["license_files"]]:
        license_path = root / str(license_entry.get("vendored_path") or license_entry.get("path"))
        if not license_path.is_file() or hashlib.sha256(license_path.read_bytes()).hexdigest() != license_entry.get("vendored_sha256", license_entry.get("sha256")):
            raise ExpressionError(f"installer toolchain legal body hash mismatch: {license_path.relative_to(root)}")
    return 3


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        inventory, sources = load_inventory(root)
    except InventoryError as exc:
        parser.error(str(exc))
    ledger_path = root / "packaging/security/dependency-licenses.json"
    policy_path = root / "packaging/security/license-policy.json"
    try:
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
        policy = json.loads(policy_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        parser.error(f"cannot read license policy inputs: {exc}")
    expected = {item["purl"]: item for item in inventory}
    declared = ledger.get("components")
    if not isinstance(declared, dict):
        parser.error("dependency license ledger has no component map")
    if ledger.get("source_locks") != sources:
        parser.error("dependency license ledger source-lock list is stale")
    missing = sorted(set(expected) - set(declared))
    extra = sorted(set(declared) - set(expected))
    if missing or extra:
        parser.error(f"license ledger does not exactly match locks (missing={len(missing)}, stale={len(extra)})")

    allowed = set(policy.get("allowed_identifiers", []))
    disallowed = set(policy.get("disallowed_identifiers", []))
    restricted = set(policy.get("restricted_identifiers", [])) | disallowed
    if allowed & restricted:
        parser.error("license policy has identifiers in both allowed and restricted sets")
    if "cargo_deny_allow" in policy:
        try:
            deny = tomllib.loads((root / "deny.toml").read_text(encoding="utf-8"))
            deny_allow = set(deny["licenses"]["allow"])
        except (OSError, KeyError, tomllib.TOMLDecodeError) as exc:
            parser.error(f"cannot read cargo-deny license policy: {exc}")
        expected_deny_allow = set(policy["cargo_deny_allow"])
        if deny_allow != expected_deny_allow:
            parser.error(f"deny.toml license allowlist differs from license-policy.json (missing={sorted(expected_deny_allow - deny_allow)}, stale={sorted(deny_allow - expected_deny_allow)})")
    exceptions = policy.get("reviewed_exceptions", {})
    failures: list[str] = []
    for purl, expression in sorted(declared.items()):
        if not isinstance(expression, str) or not expression.strip():
            failures.append(f"{purl}: unknown license")
            continue
        identifiers = {token for token in LICENSE_TOKEN.findall(expression) if token not in OPERATORS}
        exception = exceptions.get(purl)
        try:
            accepted, unknown = expression_is_allowed(expression, allowed, restricted)
        except ExpressionError as exc:
            failures.append(f"{purl}: invalid license expression {expression!r}: {exc}")
            continue
        if accepted:
            continue
        forbidden = identifiers & restricted
        if forbidden and not unknown:
            sources_for_component = {
                item["value"] for item in expected[purl].get("properties", [])
                if item.get("name") == "interactive-npcs:source-lock"
            }
            if not exception or exception.get("license") != expression or exception.get("distributed_in_base_installer") is not False or exception.get("required_source_lock") not in sources_for_component:
                failures.append(f"{purl}: disallowed license {expression}")
            continue
        if unknown:
            failures.append(f"{purl}: unreviewed identifier(s) {','.join(sorted(unknown))}")
        elif not forbidden:
            failures.append(f"{purl}: no allowed license branch in {expression}")
    if failures:
        for failure in failures:
            print(failure)
        return 1

    distribution_component_count = 0
    distribution_file_count = 0
    license_material_override_count = 0
    installer_toolchain_component_count = 0
    if (root / "packaging/security/license-material-overrides.json").is_file() or "cargo_deny_allow" in policy:
        try:
            license_material_override_count = validate_license_material_overrides(root, declared)
        except ExpressionError as exc:
            parser.error(str(exc))
    if (root / "packaging/security/distribution-components.json").is_file() or "cargo_deny_allow" in policy:
        try:
            distribution_component_count, distribution_file_count = validate_distribution_ledger(root)
        except ExpressionError as exc:
            parser.error(str(exc))
    if (root / "packaging/security/installer-toolchain-provenance.json").is_file() or "cargo_deny_allow" in policy:
        try:
            installer_toolchain_component_count = validate_installer_toolchain_provenance(root)
        except (ExpressionError, OSError, json.JSONDecodeError) as exc:
            parser.error(str(exc))

    model_root = root / "packaging/model-packs"
    for manifest_path in sorted(model_root.rglob("*.json")) if model_root.is_dir() else []:
        if re.search(r"\.(example|schema)\.json$", manifest_path.name):
            continue
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if not is_model_pack_manifest(manifest):
            continue
        error = model_provenance_error(manifest)
        if error:
            print(f"{manifest_path.relative_to(root)}: {error}")
            return 1
    runtime_path = root / "packaging/runtime-components/onnxruntime-1.22.1-cpu.json"
    if distribution_component_count:
        try:
            runtime = json.loads(runtime_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            parser.error(f"cannot read canonical ONNX Runtime coverage: {exc}")
        if runtime.get("source_revision") != "v1.22.1" or runtime.get("license", {}).get("spdx") != "MIT" or runtime.get("activation_allowed") is not False:
            parser.error("ONNX Runtime coverage must remain pinned, MIT-classified, and inactive until an artifact is admitted")
    print(json.dumps({"status": "passed", "components": len(expected), "source_locks": sources, "reviewed_exceptions": len(exceptions), "license_material_overrides": license_material_override_count, "installer_toolchain_components": installer_toolchain_component_count, "distribution_components": distribution_component_count, "static_distribution_files": distribution_file_count}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
