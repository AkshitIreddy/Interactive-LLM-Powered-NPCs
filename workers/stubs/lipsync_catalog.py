"""Validation and opt-in eligibility for the development lip-sync candidate catalog."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:  # Supports direct execution and package imports in test harnesses.
    from .contract import ContractError
except ImportError:
    from contract import ContractError

SCHEMA_VERSION = "npc.lipsync-pack-catalog/v1"
_ID = re.compile(r"^[a-z0-9][a-z0-9._-]{1,95}[a-z0-9]$")
_STATUSES = frozenset({"conditional_private_access", "public_experimental", "baseline", "deferred", "offline_only"})
_ACCESS = frozenset({"private_ngc", "public_upstream", "project_developed"})


@dataclass(frozen=True, slots=True)
class SelectionEligibility:
    eligible: bool
    reasons: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class LipSyncPackCatalog:
    raw: dict[str, Any]
    path: Path

    @classmethod
    def load(cls, path: str | Path) -> "LipSyncPackCatalog":
        catalog_path = Path(path).resolve()
        try:
            raw = json.loads(
                catalog_path.read_text(encoding="utf-8"),
                parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"non-finite number {value}")),
            )
        except (OSError, json.JSONDecodeError, RecursionError, ValueError) as exc:
            raise ContractError("invalid_lipsync_catalog", "lip-sync catalog cannot be read") from exc
        cls.validate(raw)
        return cls(raw, catalog_path)

    @staticmethod
    def validate(raw: Any) -> None:
        required = {
            "schema_version",
            "catalog_id",
            "display_name",
            "development_descriptor",
            "downloadable_catalog",
            "contains_download_locations",
            "contains_model_payloads",
            "network_access",
            "game_specific_adapters",
            "selection_policy",
            "candidates",
        }
        if not isinstance(raw, dict) or set(raw) != required:
            raise ContractError("invalid_lipsync_catalog", "lip-sync catalog fields are incompatible")
        if raw["schema_version"] != SCHEMA_VERSION:
            raise ContractError("invalid_lipsync_catalog", "lip-sync catalog schema is unsupported")
        if not isinstance(raw["catalog_id"], str) or not _ID.fullmatch(raw["catalog_id"]):
            raise ContractError("invalid_lipsync_catalog", "lip-sync catalog id is malformed")
        if not isinstance(raw["display_name"], str) or not 1 <= len(raw["display_name"]) <= 128:
            raise ContractError("invalid_lipsync_catalog", "lip-sync catalog display name is malformed")
        safety_flags = {
            "development_descriptor": True,
            "downloadable_catalog": False,
            "contains_download_locations": False,
            "contains_model_payloads": False,
            "network_access": False,
            "game_specific_adapters": False,
        }
        if any(raw.get(field) is not expected for field, expected in safety_flags.items()):
            raise ContractError(
                "invalid_lipsync_catalog",
                "development lip-sync catalog cannot download, connect, bundle models, or describe game adapters",
            )
        policy = raw["selection_policy"]
        expected_policy = {
            "user_initiated_only": True,
            "automatic_selection": False,
            "automatic_download": False,
            "explicit_candidate_id_required": True,
            "installed_verified_pack_required": True,
            "qualification_required": True,
            "fallback_chain": [],
        }
        if policy != expected_policy:
            raise ContractError("invalid_lipsync_catalog", "lip-sync selection must remain explicit and opt-in")
        candidates = raw["candidates"]
        if not isinstance(candidates, list) or not candidates or len(candidates) > 32:
            raise ContractError("invalid_lipsync_catalog", "lip-sync catalog candidates are malformed")
        seen: set[str] = set()
        for candidate in candidates:
            LipSyncPackCatalog._validate_candidate(candidate)
            candidate_id = candidate["id"]
            if candidate_id in seen:
                raise ContractError("invalid_lipsync_catalog", "lip-sync candidate ids must be unique")
            seen.add(candidate_id)

    @staticmethod
    def _validate_candidate(candidate: Any) -> None:
        fields = {
            "id",
            "display_name",
            "implementation_kind",
            "status",
            "source_access",
            "supported_platforms",
            "live_selectable",
            "offline_selectable",
            "requires_private_access",
            "requirements",
            "constraints",
        }
        if not isinstance(candidate, dict) or set(candidate) != fields:
            raise ContractError("invalid_lipsync_catalog", "lip-sync candidate fields are incompatible")
        if not isinstance(candidate["id"], str) or not _ID.fullmatch(candidate["id"]):
            raise ContractError("invalid_lipsync_catalog", "lip-sync candidate id is malformed")
        for field in ("display_name", "implementation_kind"):
            if not isinstance(candidate[field], str) or not 1 <= len(candidate[field]) <= 128:
                raise ContractError("invalid_lipsync_catalog", f"lip-sync candidate {field} is malformed")
        if candidate["status"] not in _STATUSES or candidate["source_access"] not in _ACCESS:
            raise ContractError("invalid_lipsync_catalog", "lip-sync candidate status or access is unsupported")
        if candidate["supported_platforms"] != ["windows_x64"]:
            raise ContractError("invalid_lipsync_catalog", "this catalog is Windows x64 only")
        for field in ("live_selectable", "offline_selectable", "requires_private_access"):
            if not isinstance(candidate[field], bool):
                raise ContractError("invalid_lipsync_catalog", f"lip-sync candidate {field} must be boolean")
        if candidate["status"] == "deferred" and (candidate["live_selectable"] or candidate["offline_selectable"]):
            raise ContractError("invalid_lipsync_catalog", "deferred lip-sync candidates cannot be selected")
        if candidate["status"] == "offline_only" and candidate["live_selectable"]:
            raise ContractError("invalid_lipsync_catalog", "offline-only lip-sync candidates cannot be live selected")
        if candidate["source_access"] == "private_ngc" and not candidate["requires_private_access"]:
            raise ContractError("invalid_lipsync_catalog", "private NGC candidates must require confirmed access")
        for field in ("requirements", "constraints"):
            values = candidate[field]
            if (
                not isinstance(values, list)
                or not values
                or len(values) > 32
                or any(not isinstance(value, str) or not 1 <= len(value) <= 256 for value in values)
                or len(values) != len(set(values))
            ):
                raise ContractError("invalid_lipsync_catalog", f"lip-sync candidate {field} is malformed")

    def selection_eligibility(
        self,
        candidate_id: str,
        *,
        mode: str,
        user_initiated: bool,
        installed: bool,
        verified: bool,
        qualified: bool,
        platform: str,
        private_access_confirmed: bool = False,
    ) -> SelectionEligibility:
        candidate = next((entry for entry in self.raw["candidates"] if entry["id"] == candidate_id), None)
        if candidate is None:
            return SelectionEligibility(False, ("unknown_candidate",))
        reasons: list[str] = []
        if not user_initiated:
            reasons.append("user_initiation_required")
        if mode not in {"live", "offline"}:
            reasons.append("unsupported_mode")
        elif not candidate[f"{mode}_selectable"]:
            reasons.append(f"{mode}_selection_not_allowed")
        if platform not in candidate["supported_platforms"]:
            reasons.append("unsupported_platform")
        if not installed:
            reasons.append("pack_not_installed")
        if not verified:
            reasons.append("pack_not_verified")
        if not qualified:
            reasons.append("pack_not_qualified")
        if candidate["requires_private_access"] and not private_access_confirmed:
            reasons.append("private_access_required")
        return SelectionEligibility(not reasons, tuple(reasons))
