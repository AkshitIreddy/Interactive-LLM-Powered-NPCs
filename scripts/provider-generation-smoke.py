#!/usr/bin/env python3
"""Minimal synthetic hosted-generation smoke with measured audio output.

Uses only the ignored credential file. Provider response text, credentials,
uploaded URLs, and transcript IDs are never printed or persisted.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import os
import re
import shutil
import struct
import subprocess
import time
import urllib.error
import urllib.request
import wave
from pathlib import Path


PHRASE = "The north beacon is ready."


def credentials(path: Path) -> dict[str, list[tuple[str, str]]]:
    aliases = {
        "cohere": ("cohere",),
        "elevenlabs": ("elevenlabs", "eleven labs", "elevenlab"),
        "assemblyai": ("assemblyai", "assembly ai"),
    }
    result = {provider: [] for provider in aliases}
    for raw in path.read_text(encoding="utf-8-sig").splitlines():
        line = raw.strip()
        lowered = line.lower()
        provider = next(
            (provider for provider, names in aliases.items() if any(name in lowered for name in names)),
            None,
        )
        if not provider:
            continue
        match = re.search(r"[:=]\s*([^\s].*)$", line)
        if not match:
            continue
        value = match.group(1).strip().strip("\"'")
        if len(value) < 8:
            continue
        kind = "production" if "prod" in lowered or "production" in lowered else "trial_or_unspecified"
        result[provider].append((kind, value))
    return result


def request(
    url: str,
    *,
    method: str,
    headers: dict[str, str],
    body: bytes | None = None,
    timeout: float = 30,
    maximum: int = 8 * 1024 * 1024,
) -> tuple[int, bytes, dict[str, str]]:
    req = urllib.request.Request(
        url,
        data=body,
        method=method,
        headers={**headers, "User-Agent": "Interactive-NPCs-2.0-local-media-smoke"},
    )
    with urllib.request.urlopen(req, timeout=timeout) as response:
        payload = response.read(maximum + 1)
        if len(payload) > maximum:
            raise RuntimeError("provider response exceeded smoke-test limit")
        return response.status, payload, {key.lower(): value for key, value in response.headers.items()}


def request_json(url: str, *, method: str, headers: dict[str, str], payload: dict | None = None, timeout: float = 30) -> tuple[int, dict, dict[str, str]]:
    body = None if payload is None else json.dumps(payload).encode()
    status, raw, response_headers = request(
        url,
        method=method,
        headers={"Accept": "application/json", **({"Content-Type": "application/json"} if body else {}), **headers},
        body=body,
        timeout=timeout,
        maximum=2 * 1024 * 1024,
    )
    decoded = json.loads(raw)
    if not isinstance(decoded, dict):
        raise RuntimeError("provider returned a non-object JSON response")
    return status, decoded, response_headers


def cohere_smoke(items: list[tuple[str, str]]) -> dict:
    ordered = sorted(items, key=lambda item: item[0] == "production")
    last = None
    for kind, key in ordered:
        try:
            _, models, _ = request_json(
                "https://api.cohere.com/v1/models?endpoint=chat",
                method="GET",
                headers={"Authorization": f"Bearer {key}"},
            )
            candidates = [
                model.get("name")
                for model in models.get("models", [])
                if isinstance(model, dict) and model.get("name") and not model.get("is_deprecated", False)
            ]
            if not candidates:
                raise RuntimeError("no chat model visible")
            model = candidates[0]
            started = time.perf_counter()
            _, response, _ = request_json(
                "https://api.cohere.com/v2/chat",
                method="POST",
                headers={"Authorization": f"Bearer {key}"},
                payload={
                    "model": model,
                    "messages": [{"role": "user", "content": "Reply with exactly: READY"}],
                    "max_tokens": 8,
                    "temperature": 0,
                },
            )
            elapsed = round((time.perf_counter() - started) * 1000, 1)
            content = response.get("message", {}).get("content", [])
            text = "".join(item.get("text", "") for item in content if isinstance(item, dict))
            return {
                "authenticated": True,
                "credentialClass": kind,
                "model": model,
                "latencyMs": elapsed,
                "exactReady": text.strip().upper() == "READY",
                "responseCharacters": len(text),
            }
        except urllib.error.HTTPError as error:
            last = error.code
            if error.code != 429 or kind == "production":
                break
    return {"authenticated": False, "status": "throttled" if last == 429 else "failed", "httpStatus": last}


def elevenlabs_tts(key: str, output: Path) -> dict:
    started = time.perf_counter()
    status, audio, headers = request(
        "https://api.elevenlabs.io/v1/text-to-speech/JBFqnCBsd6RMkjVDRZzb?output_format=mp3_44100_128",
        method="POST",
        headers={"xi-api-key": key, "Accept": "audio/mpeg", "Content-Type": "application/json"},
        body=json.dumps({"text": PHRASE, "model_id": "eleven_flash_v2_5"}).encode(),
        timeout=45,
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(audio)
    return {
        "authenticated": status == 200,
        "latencyMs": round((time.perf_counter() - started) * 1000, 1),
        "audioBytes": len(audio),
        "contentType": headers.get("content-type", "unknown").split(";", 1)[0],
        "characterCost": int(headers["character-cost"]) if headers.get("character-cost", "").isdigit() else None,
    }


def windows_path(path: Path) -> str:
    resolved = path.resolve()
    if os.name == "nt":
        return str(resolved)
    text = str(resolved)
    if text.startswith("/mnt/") and len(text) > 6:
        return f"{text[5].upper()}:\\{text[7:].replace('/', '\\')}"
    return text


def decode_and_measure(mp3: Path, wav: Path) -> dict:
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg and Path("/mnt/c/ffmpeg/bin/ffmpeg.exe").is_file():
        ffmpeg = "/mnt/c/ffmpeg/bin/ffmpeg.exe"
    if not ffmpeg:
        raise RuntimeError("ffmpeg is unavailable for audio verification")
    input_arg = windows_path(mp3) if ffmpeg.lower().endswith(".exe") else str(mp3)
    output_arg = windows_path(wav) if ffmpeg.lower().endswith(".exe") else str(wav)
    subprocess.run(
        [ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-i", input_arg, "-ac", "1", "-ar", "24000", "-c:a", "pcm_s16le", output_arg],
        check=True,
    )
    with wave.open(str(wav), "rb") as stream:
        channels = stream.getnchannels()
        rate = stream.getframerate()
        width = stream.getsampwidth()
        frames = stream.getnframes()
        raw = stream.readframes(frames)
    if channels != 1 or width != 2:
        raise RuntimeError("unexpected decoded PCM shape")
    samples = struct.unpack(f"<{frames}h", raw)
    peak = max(abs(value) for value in samples) / 32768 if samples else 0
    rms = math.sqrt(sum(value * value for value in samples) / max(1, len(samples))) / 32768
    clipped = sum(abs(value) >= 32767 for value in samples)
    max_delta = max((abs(right - left) for left, right in zip(samples, samples[1:])), default=0) / 32768
    peak_sample = max((abs(value) for value in samples), default=0)
    first_window = samples[: max(1, rate // 1000)]
    tail_window = samples[-max(1, rate // 50) :]
    onset_ratio = max((abs(value) for value in first_window), default=0) / peak_sample if peak_sample else 0
    tail_rms = math.sqrt(sum(value * value for value in tail_window) / max(1, len(tail_window))) / 32768
    return {
        "sampleRate": rate,
        "channels": channels,
        "durationSeconds": round(frames / rate, 3),
        "peak": round(peak, 6),
        "rms": round(rms, 6),
        "clippedSamples": clipped,
        "maxAdjacentDelta": round(max_delta, 6),
        "onsetRatio1ms": round(onset_ratio, 6),
        "tailRms20ms": round(tail_rms, 6),
        "firstSample": round(samples[0] / 32768, 6) if samples else 0,
        "lastSample": round(samples[-1] / 32768, 6) if samples else 0,
        "silent": rms < 0.0001,
    }


def assembly_transcribe(key: str, audio: Path) -> dict:
    _, upload, _ = request_json(
        "https://api.assemblyai.com/v2/upload",
        method="POST",
        headers={"Authorization": key, "Content-Type": "application/octet-stream"},
        payload=None,
    ) if False else (None, None, None)
    # Upload is binary rather than JSON-request shaped.
    _, upload_raw, _ = request(
        "https://api.assemblyai.com/v2/upload",
        method="POST",
        headers={"Authorization": key, "Content-Type": "application/octet-stream", "Accept": "application/json"},
        body=audio.read_bytes(),
        timeout=45,
        maximum=1024 * 1024,
    )
    upload = json.loads(upload_raw)
    audio_url = upload.get("upload_url")
    if not isinstance(audio_url, str):
        raise RuntimeError("AssemblyAI upload did not return a URL")
    _, created, _ = request_json(
        "https://api.assemblyai.com/v2/transcript",
        method="POST",
        headers={"Authorization": key},
        payload={"audio_url": audio_url, "language_code": "en"},
        timeout=30,
    )
    transcript_id = created.get("id")
    if not isinstance(transcript_id, str):
        raise RuntimeError("AssemblyAI did not return a transcript id")
    started = time.perf_counter()
    final = None
    for _ in range(60):
        _, state, _ = request_json(
            f"https://api.assemblyai.com/v2/transcript/{transcript_id}",
            method="GET",
            headers={"Authorization": key},
            timeout=20,
        )
        if state.get("status") in ("completed", "error"):
            final = state
            break
        time.sleep(1)
    if final is None:
        raise RuntimeError("AssemblyAI transcription timed out")
    text = final.get("text") or ""
    normalized = re.sub(r"[^a-z]", "", text.lower())
    expected = re.sub(r"[^a-z]", "", PHRASE.lower())
    deleted = False
    try:
        status, _, _ = request(
            f"https://api.assemblyai.com/v2/transcript/{transcript_id}",
            method="DELETE",
            headers={"Authorization": key},
            timeout=20,
            maximum=1024 * 1024,
        )
        deleted = status in (200, 204)
    except urllib.error.HTTPError:
        deleted = False
    return {
        "authenticated": True,
        "status": final.get("status"),
        "latencyMs": round((time.perf_counter() - started) * 1000, 1),
        "textMatchesFixture": normalized == expected,
        "confidence": round(float(final.get("confidence") or 0), 4),
        "remoteTranscriptDeleted": deleted,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--credentials", type=Path, default=Path(".secrets/Commonly used Keys.txt"))
    parser.add_argument("--output-dir", type=Path, default=Path("artifacts/provider-smoke"))
    args = parser.parse_args()
    keys = credentials(args.credentials)
    for provider in ("cohere", "elevenlabs", "assemblyai"):
        if not keys[provider]:
            raise SystemExit(f"missing {provider} credential")

    output = args.output_dir
    mp3 = output / "elevenlabs-fixture.mp3"
    wav = output / "elevenlabs-fixture.wav"
    report = {
        "schemaVersion": 1,
        "checkedAtUtc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "fixtureText": PHRASE,
        "containsCredentialValues": False,
        "cohere": cohere_smoke(keys["cohere"]),
        "elevenlabs": elevenlabs_tts(keys["elevenlabs"][0][1], mp3),
    }
    report["audioMetrics"] = decode_and_measure(mp3, wav)
    report["assemblyai"] = assembly_transcribe(keys["assemblyai"][0][1], mp3)
    output.mkdir(parents=True, exist_ok=True)
    report_path = output / "live-generation.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, sort_keys=True))
    if not report["cohere"].get("authenticated") or not report["elevenlabs"].get("authenticated") or not report["assemblyai"].get("authenticated"):
        raise SystemExit(1)
    if report["audioMetrics"]["silent"] or report["audioMetrics"]["clippedSamples"]:
        raise SystemExit(1)
    if not report["assemblyai"]["textMatchesFixture"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
