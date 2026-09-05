#!/usr/bin/env python3
"""Run one redacted structured runtime qualification with bounded provider calls."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
from pathlib import Path


def read_key(path: Path, expected_label: str) -> str | None:
    for raw in path.read_text(encoding="utf-8", errors="strict").splitlines():
        line = raw.strip()
        if not line or line.startswith(("#", ";", "//")):
            continue
        positions = [position for position in (line.find("="), line.find(":")) if position >= 0]
        if not positions:
            continue
        split = min(positions)
        label = line[:split].strip().strip("\"'`").lower().replace(" ", "_").replace("-", "_")
        if label != expected_label:
            continue
        value = line[split + 1 :].strip().strip("\"'")
        if len(value) >= 8 and not any(ord(character) < 32 for character in value):
            return value
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keys-file", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument(
        "--mode",
        choices=("mistral-host-state", "groq-cartesia-component-chain"),
        default="mistral-host-state",
    )
    parser.add_argument(
        "--target-dir",
        type=Path,
        default=Path(r"E:\temp\InteractiveNPCs\cargo-target-native-slice"),
    )
    args = parser.parse_args()
    labels = (
        ("mistral",)
        if args.mode == "mistral-host-state"
        else ("groq", "cartesia")
    )
    keys = {label: read_key(args.keys_file, label) for label in labels}
    missing = [label for label, value in keys.items() if value is None]
    if missing:
        parser.error(f"Missing credential labels: {', '.join(missing)}")
    if args.report.exists():
        parser.error(f"report already exists: {args.report}")
    args.report.parent.mkdir(parents=True, exist_ok=True)

    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(args.target_dir)
    if args.mode == "mistral-host-state":
        environment["MISTRAL_API_KEY"] = keys["mistral"]
        environment["STRUCTURED_NORMAL_ROUTE_REPORT"] = str(args.report)
        test_name = "selected_mistral_turn_uses_speech_first_schema_without_audio_claims"
    else:
        environment["GROQ_API_KEY"] = keys["groq"]
        environment["CARTESIA_API_KEY"] = keys["cartesia"]
        environment["GROQ_CARTESIA_STREAMING_REPORT"] = str(args.report)
        test_name = (
            "groq_qwen_speech_first_field_dispatches_one_cartesia_request_without_playback"
        )
    repo = Path(__file__).resolve().parents[2]
    process = subprocess.run(
        [
            "cargo",
            "test",
            "--quiet",
            "--manifest-path",
            "apps/runtime-host/Cargo.toml",
            "--features",
            "test-fixture-vault",
            "--test",
            "live_structured_normal_route",
            "--",
            "--ignored",
            "--exact",
            test_name,
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
    secret_values = [value for value in keys.values() if value is not None]
    if any(secret in combined for secret in secret_values):
        print(json.dumps({"status": "credential-output-blocked"}))
        return 1
    if process.returncode != 0 or not args.report.is_file():
        print(
            json.dumps(
                {
                    "status": "normal-route-failed",
                    "exitCode": process.returncode,
                    "diagnostic": combined[-3000:],
                },
                indent=2,
            )
        )
        return 1
    report = json.loads(args.report.read_text(encoding="utf-8"))
    if any(secret in json.dumps(report, sort_keys=True) for secret in secret_values):
        print(json.dumps({"status": "credential-report-blocked"}))
        return 1
    if args.mode == "mistral-host-state":
        summary = {
            "status": "passed",
            "mode": args.mode,
            "report": str(args.report),
            "providerId": report["providerId"],
            "modelId": report["modelId"],
            "routeFormat": report["routeFormat"],
            "elapsedMs": report["elapsedMs"],
            "structuredResponseValidated": report["structuredResponseValidated"],
            "deliveredSubtitleSentences": report["deliveredSubtitleSentences"],
            "credentialMatches": 0,
            "audioDeliveryClaimed": report["audioDeliveryClaimed"],
        }
    else:
        summary = {
            "status": "passed",
            "mode": args.mode,
            "report": str(args.report),
            "llmProviderId": report["llm"]["providerId"],
            "llmModelId": report["llm"]["modelId"],
            "ttsProviderId": report["tts"]["providerId"],
            "ttsModelId": report["tts"]["modelId"],
            "requestToSpokenFieldValidatedUs": report["llm"][
                "requestToSpokenFieldValidatedUs"
            ],
            "validatedFieldToProviderFirstPcmUs": report["tts"][
                "validatedFieldToProviderFirstPcmUs"
            ],
            "validatedFieldToBridgeFirstPcmUs": report["tts"][
                "validatedFieldToBridgeFirstPcmUs"
            ],
            "requestToBridgeFirstPcmUs": report["combined"][
                "requestToBridgeFirstPcmUs"
            ],
            "credentialMatches": 0,
            "audioDeliveryClaimed": report["audioDeliveryClaimed"],
        }
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
