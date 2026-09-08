#!/usr/bin/env python3
"""Run the ignored Cartesia RuntimeTtsBridge qualification without playback."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import subprocess
from pathlib import Path

from provider_timing_alternatives import resolve_credentials


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--credentials", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument(
        "--target-dir",
        type=Path,
        default=Path(r"E:\temp\InteractiveNPCs\cargo-target"),
    )
    args = parser.parse_args()
    credentials, _ = resolve_credentials(args.credentials)
    key = credentials.get("cartesia")
    if not key:
        parser.error("Cartesia credential is missing")
    if args.report.exists():
        parser.error(f"report already exists: {args.report}")
    args.report.parent.mkdir(parents=True, exist_ok=True)

    repo = Path(__file__).resolve().parents[2]
    environment = os.environ.copy()
    environment.update(
        {
            "CARTESIA_API_KEY": key,
            "CARTESIA_RUNTIME_BRIDGE_REPORT": str(args.report),
            "CARGO_TARGET_DIR": str(args.target_dir),
        }
    )
    process = subprocess.run(
        [
            "cargo",
            "test",
            "--quiet",
            "--manifest-path",
            "apps/runtime-host/Cargo.toml",
            "--test",
            "live_cartesia_runtime_bridge",
            "--",
            "--ignored",
            "--exact",
            "pooled_cartesia_is_measured_across_same_turn_sentences_and_a_second_turn",
        ],
        cwd=repo,
        env=environment,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=180,
        check=False,
    )
    combined = process.stdout + process.stderr
    if key in combined:
        print(json.dumps({"status": "credential-output-blocked"}))
        return 1
    if process.returncode != 0 or not args.report.is_file():
        print(
            json.dumps(
                {
                    "status": "runtime-bridge-qualification-failed",
                    "exitCode": process.returncode,
                    "diagnostic": combined[-3000:],
                },
                indent=2,
            )
        )
        return 1
    report = json.loads(args.report.read_text(encoding="utf-8"))
    serialized = json.dumps(report, sort_keys=True)
    if key in serialized:
        print(json.dumps({"status": "credential-report-blocked"}))
        return 1
    print(
        json.dumps(
            {
                "status": "passed",
                "checkedAtUtc": dt.datetime.now(dt.timezone.utc)
                .replace(microsecond=0)
                .isoformat()
                .replace("+00:00", "Z"),
                "report": str(args.report),
                "firstRequestToBridgeAudioMs": (
                    report["firstSameTurnSentence"]["bridgeFirstAudioUs"]
                    - report["firstSameTurnSentence"]["requestUs"]
                )
                / 1000,
                "secondSameTurnRequestToBridgeAudioMs": (
                    report["secondSameTurnSentence"]["bridgeFirstAudioUs"]
                    - report["secondSameTurnSentence"]["requestUs"]
                )
                / 1000,
                "secondTurnRequestToBridgeAudioMs": (
                    report["secondTurnSentence"]["bridgeFirstAudioUs"]
                    - report["secondTurnSentence"]["requestUs"]
                )
                / 1000,
                "providerToBridgeAudioUs": [
                    report["firstSameTurnSentence"]["providerToBridgeFirstAudioUs"],
                    report["secondSameTurnSentence"]["providerToBridgeFirstAudioUs"],
                    report["secondTurnSentence"]["providerToBridgeFirstAudioUs"],
                ],
                "freshConnections": report["freshConnections"],
                "reusedConnections": report["reusedConnections"],
                "credentialMatches": 0,
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
