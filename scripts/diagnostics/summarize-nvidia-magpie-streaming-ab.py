#!/usr/bin/env python3
"""Build a sanitized comparison receipt from Magpie latency measurements."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import statistics


EXPECTED_OFFICIAL_CONTRACT = {
    "authority": "grpc.nvcf.nvidia.com:443",
    "functionId": "877104f7-e885-42b9-8de8-f6e4c6303969",
    "method": "/nvidia.riva.tts.RivaSpeechSynthesis/SynthesizeOnline",
    "voice": "Magpie-Multilingual.EN-US.Aria",
    "languageCode": "en-US",
    "sampleRateHz": 22_050,
    "encoding": "LINEAR_PCM",
    "textSha256": "9fc89b7c00f0d2c44059ee4321ccc62f450263458d3cafda1f3c3422f82ac8b4",
}


def load(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise RuntimeError(f"invalid_metric:{path.name}")
    return value


def numbers(values) -> list[float]:
    return [float(value) for value in values if value is not None]


def summary(values: list[float]) -> dict[str, float | int | None]:
    return {
        "count": len(values),
        "minMs": round(min(values), 1) if values else None,
        "medianMs": round(statistics.median(values), 1) if values else None,
        "maxMs": round(max(values), 1) if values else None,
    }


def validate_official_contract(metric: dict, label: str) -> None:
    mismatches = [
        key
        for key, expected in EXPECTED_OFFICIAL_CONTRACT.items()
        if metric.get(key) != expected
    ]
    if mismatches:
        raise RuntimeError(f"{label}_request_contract_mismatch:{','.join(mismatches)}")
    if metric.get("containsCredentialValues") is not False:
        raise RuntimeError(f"{label}_credential_boundary_missing")
    if metric.get("containsProviderResponseText") is not False:
        raise RuntimeError(f"{label}_response_text_boundary_missing")


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--historical-repo", type=Path, required=True)
    parser.add_argument("--fresh-repo", type=Path, required=True)
    parser.add_argument("--official", type=Path, required=True)
    parser.add_argument("--official-variants", type=Path, required=True)
    parser.add_argument("--historical-http", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise RuntimeError("refusing_to_overwrite_metrics")

    historical_repo = load(args.historical_repo)
    fresh_repo = load(args.fresh_repo)
    official = load(args.official)
    official_variants = load(args.official_variants)
    historical_http = load(args.historical_http)
    validate_official_contract(official, "official")
    validate_official_contract(official_variants, "official_variants")

    historical_first = numbers(
        row.get("firstAudioMs") for row in historical_repo.get("utterances", [])
    )
    historical_headers = numbers(
        row.get("responseHeadersMs") for row in historical_repo.get("utterances", [])
    )
    fresh_first = numbers(row.get("firstAudioMs") for row in fresh_repo.get("utterances", []))
    fresh_headers = numbers(
        row.get("responseHeadersMs") for row in fresh_repo.get("utterances", [])
    )
    official_first = numbers(
        row.get("firstNonEmptyPcmMs") for row in official.get("runs", [])
    )
    variants = official_variants.get("wireVariants", [])
    variant_first = numbers(row.get("firstNonEmptyPcmMs") for row in variants)
    variant_upstream = numbers(row.get("upstreamServiceTimeMs") for row in variants)
    http_totals = numbers(row.get("latencyMs") for row in historical_http.get("utterances", []))

    if not historical_first or len(historical_first) != len(historical_headers):
        raise RuntimeError("historical_repository_samples_incomplete")
    if not fresh_first or len(fresh_first) != len(fresh_headers):
        raise RuntimeError("fresh_repository_samples_incomplete")
    if not official_first or not variant_first or not http_totals:
        raise RuntimeError("official_or_http_samples_incomplete")

    historical_header_to_pcm = [
        round(first - header, 1)
        for first, header in zip(historical_first, historical_headers, strict=True)
    ]
    fresh_header_to_pcm = [
        round(first - header, 1)
        for first, header in zip(fresh_first, fresh_headers, strict=True)
    ]
    old_median = statistics.median(historical_first)
    official_median = statistics.median(official_first)
    if official_median <= 0:
        raise RuntimeError("official_first_pcm_median_invalid")
    if min(historical_first) < 10_000 or max(fresh_first) >= 1_000 or max(official_first) >= 1_000:
        raise RuntimeError("input_measurements_do_not_support_comparison_claims")

    receipt = {
        "schemaVersion": 1,
        "comparison": "repository-tonic-vs-official-riva-magpie-synthesize-online",
        "identicalRequestContract": {
            "authority": "grpc.nvcf.nvidia.com:443",
            "functionId": "877104f7-e885-42b9-8de8-f6e4c6303969",
            "method": "/nvidia.riva.tts.RivaSpeechSynthesis/SynthesizeOnline",
            "textSha256": "9fc89b7c00f0d2c44059ee4321ccc62f450263458d3cafda1f3c3422f82ac8b4",
            "textCharacters": 26,
            "voice": "Magpie-Multilingual.EN-US.Aria",
            "languageCode": "en-US",
            "sampleRateHz": 22050,
            "encoding": "LINEAR_PCM",
            "zeroShotDataPresent": False,
        },
        "measurements": {
            "historicalRepositoryFirstPcm": summary(historical_first),
            "historicalRepositoryResponseHeaders": summary(historical_headers),
            "historicalRepositoryHeaderToPcmGap": summary(historical_header_to_pcm),
            "freshRepositoryFirstPcm": summary(fresh_first),
            "freshRepositoryResponseHeaders": summary(fresh_headers),
            "freshRepositoryHeaderToPcmGap": summary(fresh_header_to_pcm),
            "freshOfficialFirstPcm": summary(official_first),
            "freshOfficialWireVariantFirstPcm": summary(variant_first),
            "freshOfficialWireVariantUpstreamServiceHeader": summary(variant_upstream),
            "historicalOfficialHttpOfflineTotal": summary(http_totals),
            "historicalToFreshOfficialMedianRatio": round(old_median / official_median, 1),
        },
        "wireVariants": [
            {
                "name": row.get("name"),
                "requestIdPresent": row.get("requestIdPresent"),
                "grpcTimeoutPresent": row.get("grpcTimeoutPresent"),
                "singleExplicitMetadataLayer": row.get("singleExplicitMetadataLayer"),
                "responseHeadersMs": row.get("responseHeadersMs"),
                "firstNonEmptyPcmMs": row.get("firstNonEmptyPcmMs"),
                "upstreamServiceTimeMs": row.get("upstreamServiceTimeMs"),
                "emptyMessageCount": row.get("emptyMessageCount"),
            }
            for row in variants
        ],
        "directlyEstablished": [
            "The historical 27-36 second repository delay occurred before response headers; first PCM followed headers by only tens of milliseconds.",
            "The official Riva client and the unchanged repository Tonic adapter both returned sub-second first PCM in the fresh comparison window.",
            "Request ID presence, a 60-second grpc-timeout header, and single explicit metadata injection did not reproduce the delay.",
            "Fresh official streams contained seven or eight non-empty PCM messages and no empty leading messages.",
            "Historical HTTP offline synthesis was also slow, so the old delay was not isolated to the repository gRPC client.",
        ],
        "notEstablished": [
            "The exact NVIDIA-side cause of the historical delay; cold capacity, queueing, and gateway/backend state were not separately observable.",
            "A latency service-level guarantee from the private-evaluation endpoint.",
            "Physical speaker playback or native broker drain behavior.",
        ],
        "sourceFiles": [
            args.historical_repo.name,
            args.fresh_repo.name,
            args.official.name,
            args.official_variants.name,
            args.historical_http.name,
        ],
        "officialSources": [
            "https://build.nvidia.com/nvidia/magpie-tts-multilingual/api",
            "https://github.com/nvidia-riva/python-clients/blob/main/scripts/tts/talk.py",
            "https://github.com/nvidia-riva/python-clients/blob/main/riva/client/tts.py",
        ],
        "containsCredentialValues": False,
        "containsProviderResponseText": False,
        "audioPersistedByIndependentProbe": False,
        "playbackAttempted": False,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as stream:
        stream.write(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"ok": True, "output": str(args.output)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
