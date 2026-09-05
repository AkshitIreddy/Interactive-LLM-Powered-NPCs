#!/usr/bin/env python3
"""Create one bounded, stock-voice Magpie WAV for a private held-out proof.

The report records the utterance hash and audio metrics, but never persists the
credential or the provider response text.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
from pathlib import Path
import time


VOICE = "Magpie-Multilingual.EN-US.Jason"
BASE = "https://877104f7-e885-42b9-8de8-f6e4c6303969.invocation.api.nvcf.nvidia.com"


def load_provider_module(repo: Path):
    path = repo / "scripts" / "provider-nvidia-smoke.py"
    spec = importlib.util.spec_from_file_location("provider_nvidia_smoke", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load the repository NVIDIA provider harness")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--credentials", type=Path, required=True)
    parser.add_argument("--text", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or not args.text.strip() or len(args.text) > 240:
        raise RuntimeError("use a fresh output directory and 1 through 240 text characters")
    args.output.mkdir(parents=True)
    repo = Path(__file__).resolve().parents[2]
    provider = load_provider_module(repo)
    key = provider.read_nvidia_key(args.credentials)

    status, raw, _ = provider.call_bytes(
        f"{BASE}/v1/audio/list_voices", key, method="GET", timeout=30
    )
    payload = json.loads(raw)
    candidates: list[str] = []
    values = payload.get("voices") if isinstance(payload, dict) else payload
    if isinstance(payload, dict) and not isinstance(values, list):
        values = [
            voice
            for group in payload.values()
            if isinstance(group, dict) and isinstance(group.get("voices"), list)
            for voice in group["voices"]
        ]
    if isinstance(values, list):
        for item in values:
            if isinstance(item, str):
                candidates.append(item)
            elif isinstance(item, dict):
                value = item.get("name") or item.get("voice") or item.get("voice_id")
                if isinstance(value, str):
                    candidates.append(value)
    stock = provider.stock_en_us_voices(candidates)
    if status != 200 or VOICE not in stock:
        raise RuntimeError("the exact stock Jason voice was not discovered")

    body, content_type = provider.multipart(
        {
            "text": args.text,
            "language": "en-US",
            "voice": VOICE,
            "encoding": "LINEAR_PCM",
            "sample_rate_hz": "22050",
        }
    )
    started = time.perf_counter()
    synth_status, audio, headers = provider.call_bytes(
        f"{BASE}/v1/audio/synthesize",
        key,
        method="POST",
        body=body,
        content_type=content_type,
        timeout=60,
    )
    wav = args.output / "nvidia-magpie-jason-heldout.wav"
    wav.write_bytes(audio)
    metrics = provider.measure_wav(wav)
    ok = synth_status == 200 and not metrics["silent"] and metrics["clippedSamples"] == 0
    report = {
        "schema": "interactive-npcs-private-jason-heldout/v1",
        "checkedAtUtc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "ok": ok,
        "httpStatus": synth_status,
        "voice": VOICE,
        "voiceCloningUsed": False,
        "stockEnglishVoices": len(stock),
        "textSha256": hashlib.sha256(args.text.encode("utf-8")).hexdigest(),
        "textWordCount": len(args.text.split()),
        "containsCredentialValues": False,
        "containsProviderResponseText": False,
        "contentType": headers.get("content-type", "unknown").split(";", 1)[0],
        "latencyMs": round((time.perf_counter() - started) * 1000, 1),
        "wav": str(wav),
        "wavSha256": hashlib.sha256(audio).hexdigest(),
        "audioMetrics": metrics,
    }
    (args.output / "report.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(report, sort_keys=True))
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
