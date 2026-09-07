#!/usr/bin/env python3
"""Build a credential-free index over immutable hosted-TTS evidence reports."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any

from provider_timing_alternatives import DEFAULT_OUTPUT, atomic_json, resolve_credentials, utc_now


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"report must be an object: {path.name}")
    return value


def report_entry(path: Path, report: dict[str, Any]) -> dict[str, Any]:
    raw = path.read_bytes()
    return {
        "file": path.name,
        "checkedAtUtc": report.get("checkedAtUtc"),
        "purpose": report.get("purpose"),
        "bytes": len(raw),
        "sha256": hashlib.sha256(raw).hexdigest(),
    }


def websocket_comparison(reports: list[dict[str, Any]]) -> list[dict[str, Any]]:
    latest_rows: dict[str, dict[str, Any]] = {}
    for report in reports:
        for profile in report.get("profiles", []):
            runs = profile.get("runs", [])
            if not runs:
                continue
            row = {
                    "id": profile.get("id"),
                    "provider": profile.get("provider"),
                    "model": profile.get("model"),
                    "voice": profile.get("voice"),
                    "alignment": profile.get("alignment"),
                    "connectionSetupMs": profile.get("connectionSetupMs"),
                    "firstDecodedPcmMsSamples": [run.get("requestToFirstDecodedPcmMs") for run in runs],
                    "first20msPcmMsSamples": [run.get("requestToFirst20msPcmMs") for run in runs],
                    "completeMsSamples": [run.get("requestToCompleteMs") for run in runs],
                    "audioDurationSecondsSamples": [run.get("audio", {}).get("durationSeconds") for run in runs],
                    "audioEventCountSamples": [run.get("audioEvents") for run in runs],
                    "phonemeCountSamples": [run.get("alignment", {}).get("phonemeCount") for run in runs],
                    "visemeCountSamples": [run.get("alignment", {}).get("visemeCount") for run in runs],
                    "allRunsUsable": all(run.get("status") == "usable-audio" for run in runs),
                }
            latest_rows[str(profile.get("id"))] = row
    rows = list(latest_rows.values())

    def first_pcm_sort_key(row: dict[str, Any]) -> float:
        values = [
            float(value)
            for value in row["firstDecodedPcmMsSamples"]
            if value is not None
        ]
        return min(values) if values else float("inf")

    rows.sort(key=first_pcm_sort_key)
    return rows


def http_comparison(report: dict[str, Any]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for profile in report.get("profiles", []):
        runs = profile.get("runs", [])
        rows.append(
            {
                "id": profile.get("id"),
                "firstDecodedPcmMsSamples": [run.get("requestToFirstDecodedPcmMs") for run in runs],
                "completeMsSamples": [run.get("requestToCompleteMs") for run in runs],
                "transportReadCountSamples": [run.get("transportReads") for run in runs],
                "decodedAudioFragmentCountSamples": [run.get("decodedAudioFragments") for run in runs],
                "allRunsUsable": bool(runs) and all(run.get("status") == "usable-audio" for run in runs),
            }
        )
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence-dir", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--credentials", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    evidence_dir = args.evidence_dir
    ws_paths = sorted(evidence_dir.glob("provider-websocket-warm-*.json"))
    http_path = evidence_dir / "http-stream-repeat2-20260905.json"
    account_path = evidence_dir / "elevenlabs-account-probe-20260905T043535Z.json"
    cancel_paths = sorted(evidence_dir.glob("provider-cancellation-reuse-*.json"))
    rust_adapter_paths = sorted(evidence_dir.glob("rust-production-adapter-qualification-*.json"))
    if (
        len(ws_paths) < 3
        or not http_path.is_file()
        or not account_path.is_file()
        or not cancel_paths
        or not rust_adapter_paths
    ):
        parser.error("expected immutable evidence set is incomplete")
    selected_paths = [
        *ws_paths,
        http_path,
        account_path,
        cancel_paths[-1],
        rust_adapter_paths[-1],
    ]
    reports = {path: load_json(path) for path in selected_paths}

    credentials, _ = resolve_credentials(args.credentials)
    credential_values = [value.encode("utf-8") for value in credentials.values() if value]
    scanned_paths = [path for path in evidence_dir.rglob("*") if path.is_file()]
    secret_matches = 0
    for path in scanned_paths:
        raw = path.read_bytes()
        secret_matches += sum(value in raw for value in credential_values)

    ws_reports = [reports[path] for path in ws_paths]
    authoritative_report_by_profile: dict[str, str] = {}
    superseded_reports: list[dict[str, str]] = []
    for path in ws_paths:
        for profile in reports[path].get("profiles", []):
            profile_id = profile.get("id")
            if not isinstance(profile_id, str):
                continue
            previous = authoritative_report_by_profile.get(profile_id)
            if previous:
                superseded_reports.append(
                    {"file": previous, "profile": profile_id, "supersededBy": path.name}
                )
            authoritative_report_by_profile[profile_id] = path.name
    account = reports[account_path].get("accountProbes", {}).get("elevenlabs", {})
    cancellation = reports[cancel_paths[-1]]
    rust_adapter = reports[rust_adapter_paths[-1]]
    index = {
        "schemaVersion": 1,
        "builtAtUtc": utc_now(),
        "purpose": "immutable-hosted-stock-tts-evidence-index",
        "containsCredentialValues": False,
        "credentialAbsenceVerification": {
            "method": "exact in-memory match against each resolved credential value; values never persisted",
            "filesScanned": len(scanned_paths),
            "matchesFound": secret_matches,
            "passed": secret_matches == 0,
        },
        "sampleLimit": "Two persistent-connection turns per profile are integration measurements, not service latency guarantees.",
        "reports": [report_entry(path, reports[path]) for path in selected_paths],
        "authoritativeReportByProfile": dict(sorted(authoritative_report_by_profile.items())),
        "supersededReports": superseded_reports,
        "persistentWebSocketComparison": websocket_comparison(ws_reports),
        "httpStreamingComparison": http_comparison(reports[http_path]),
        "cancellationAndReuse": {
            "cartesia": cancellation.get("cartesia"),
            "inworld": cancellation.get("inworld"),
        },
        "productionRustAdapterQualification": rust_adapter,
        "elevenlabsAccountProbe": account,
        "notLiveTested": [
            {"provider": "openai", "reason": "credential-missing"},
            {"provider": "gemini", "reason": "credential-missing"},
            {"provider": "groq", "reason": "credential-missing"},
            {"provider": "elevenlabs", "reason": "account-quota-exhausted; synthesis intentionally not retried"},
        ],
        "recommendation": {
            "latencyPrimary": "cartesia-sonic36-phonemes",
            "directVisemeOption": "inworld-flash-word-async",
            "bargeInOption": "deepgram-flux",
            "integration": (
                "Keep one provider-neutral raw PCM/alignment sink and one persistent connection per provider. "
                "Use unique turn epochs and discard late frames locally for every provider. Map Cartesia phonemes "
                "to the native mouth set; consume Inworld visemeSymbol directly; stop Flux only after "
                "SpeechMetadata on normal completion and use Interrupt/SpeechInterrupted for barge-in."
            ),
        },
        "currentPlanAndTerms": {
            "cartesia": "20,000 free credits monthly (about 27 Sonic-3.6 minutes); commercial license begins with Pro ($5).",
            "deepgram": "$200 new-account credit; Flux promotional free period ends 2026-09-12; private qualification only because published terms restrict competitive benchmarking.",
            "inworld": "Up to 70 On-Demand minutes free with commercial use; Flash $15/M characters and TTS-2 $25/M characters.",
            "elevenlabs": "10,000-character free plan is exhausted for the current account; free output is non-commercial.",
        },
    }
    output = args.output or evidence_dir / "provider-alternatives-index.json"
    if output.exists():
        parser.error(f"index file already exists: {output}")
    atomic_json(output, index)
    print(
        json.dumps(
            {
                "index": str(output),
                "reportsIndexed": len(index["reports"]),
                "profilesCompared": len(index["persistentWebSocketComparison"]),
                "credentialMatchesFound": secret_matches,
                "fastestPersistentProfile": (
                    index["persistentWebSocketComparison"][0]["id"]
                    if index["persistentWebSocketComparison"]
                    else None
                ),
            },
            indent=2,
        )
    )
    return 0 if secret_matches == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
