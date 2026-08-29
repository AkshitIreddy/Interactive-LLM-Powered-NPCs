"""Deterministic modality engines used only by the protocol conformance workers."""

from __future__ import annotations

import base64
import hashlib
import math
import re
import struct
from collections.abc import Callable, Iterator
from typing import Any

try:  # Supports direct execution and package imports in test harnesses.
    from .contract import (
        ContractError,
        MAX_EMBEDDING_DIMENSIONS,
        MOUTH_RESIDUAL_CONTRACT_VERSION,
        MouthResidualProposalV1,
        MouthResidualRequestV1,
        validate_string_list,
        validate_text,
    )
except ImportError:
    from contract import (
        ContractError,
        MAX_EMBEDDING_DIMENSIONS,
        MOUTH_RESIDUAL_CONTRACT_VERSION,
        MouthResidualProposalV1,
        MouthResidualRequestV1,
        validate_string_list,
        validate_text,
    )

Event = tuple[str, dict[str, Any]]
Cancelled = Callable[[], bool]
_WORDS = re.compile(r"[\w'-]+", re.UNICODE)


def _stable_digest(namespace: str, value: str) -> bytes:
    return hashlib.sha256(f"{namespace}\0{value}".encode("utf-8")).digest()


def _tokens(text: str, maximum: int) -> list[str]:
    words = _WORDS.findall(text)
    if not words:
        return ["…"]
    return words[:maximum]


def llm(payload: dict[str, Any], cancelled: Cancelled) -> Iterator[Event]:
    if "messages" in payload:
        messages = payload["messages"]
        if not isinstance(messages, list) or not messages or len(messages) > 256:
            raise ContractError("invalid_payload", "messages must contain 1 to 256 entries")
        source = ""
        for message in reversed(messages):
            if not isinstance(message, dict):
                raise ContractError("invalid_payload", "each message must be an object")
            role = message.get("role")
            if role not in {"system", "user", "assistant", "tool"}:
                raise ContractError("invalid_payload", "message role is unsupported")
            content = validate_text(message.get("content"), "message.content", allow_empty=True)
            if role == "user" and content:
                source = content
                break
        if not source:
            source = validate_text(messages[-1].get("content"), "message.content", allow_empty=True)
    else:
        source = validate_text(payload.get("prompt"), "prompt")
    maximum = payload.get("max_tokens", 48)
    if isinstance(maximum, bool) or not isinstance(maximum, int) or not 1 <= maximum <= 512:
        raise ContractError("invalid_payload", "max_tokens must be between 1 and 512")
    digest = _stable_digest("llm", source).hex()[:8]
    response = f"NPC fixture {digest}: I heard {source.strip()}"
    emitted: list[str] = []
    for token in _tokens(response, maximum):
        if cancelled():
            return
        emitted.append(token)
        yield "token", {"text": token + " ", "token_index": len(emitted) - 1}
    text = " ".join(emitted)
    yield "llm_result", {
        "text": text,
        "finish_reason": "stop",
        "usage": {"input_units": len(_tokens(source, 100_000)), "output_units": len(emitted)},
        "deterministic": True,
    }


def stt(payload: dict[str, Any], cancelled: Cancelled) -> Iterator[Event]:
    transcript = validate_text(payload.get("transcript_hint"), "transcript_hint")
    language = payload.get("language", "en")
    language = validate_text(language, "language")
    words = _tokens(transcript, 4_096)
    midpoint = max(1, len(words) // 2)
    if not cancelled():
        yield "partial_transcript", {"text": " ".join(words[:midpoint]), "stable": False, "language": language}
    if cancelled():
        return
    duration_ms = max(120, len(words) * 180)
    yield "transcript", {
        "text": " ".join(words),
        "language": language,
        "confidence": 1.0,
        "duration_ms": duration_ms,
        "words": [
            {"text": word, "start_ms": index * 180, "end_ms": (index + 1) * 180, "confidence": 1.0}
            for index, word in enumerate(words)
        ],
        "deterministic": True,
    }


def tts(payload: dict[str, Any], cancelled: Cancelled) -> Iterator[Event]:
    text = validate_text(payload.get("text"), "text")
    sample_rate = payload.get("sample_rate_hz", 16_000)
    if sample_rate not in {16_000, 22_050, 24_000, 48_000}:
        raise ContractError("invalid_payload", "sample_rate_hz is unsupported")
    duration_ms = min(1_000, max(120, len(_tokens(text, 4_096)) * 90))
    sample_count = sample_rate * duration_ms // 1_000
    frequency = 180 + (_stable_digest("tts", text)[0] % 80)
    samples = bytearray()
    for index in range(sample_count):
        if index % 2_048 == 0 and cancelled():
            return
        fade = min(1.0, index / max(1, sample_rate // 100), (sample_count - index) / max(1, sample_rate // 100))
        value = int(math.sin(2 * math.pi * frequency * index / sample_rate) * 4_096 * fade)
        samples.extend(struct.pack("<h", value))
    chunk_size = 16_384
    for chunk_index, start in enumerate(range(0, len(samples), chunk_size)):
        if cancelled():
            return
        chunk = bytes(samples[start : start + chunk_size])
        yield "audio_chunk", {
            "chunk_index": chunk_index,
            "pcm_s16le_b64": base64.b64encode(chunk).decode("ascii"),
            "sample_rate_hz": sample_rate,
            "channels": 1,
        }
    words = _tokens(text, 4_096)
    yield "alignment", {
        "words": [
            {"text": word, "start_ms": index * duration_ms // len(words), "end_ms": (index + 1) * duration_ms // len(words)}
            for index, word in enumerate(words)
        ]
    }
    yield "tts_result", {
        "duration_ms": duration_ms,
        "sample_rate_hz": sample_rate,
        "channels": 1,
        "sample_count": sample_count,
        "deterministic": True,
    }


def embedding(payload: dict[str, Any], cancelled: Cancelled) -> Iterator[Event]:
    texts = validate_string_list(payload.get("texts"), "texts")
    dimensions = payload.get("dimensions", 16)
    if isinstance(dimensions, bool) or not isinstance(dimensions, int) or not 1 <= dimensions <= MAX_EMBEDDING_DIMENSIONS:
        raise ContractError("invalid_payload", f"dimensions must be between 1 and {MAX_EMBEDDING_DIMENSIONS}")
    if len(texts) * dimensions > 16_384:
        raise ContractError(
            "inline_result_too_large",
            "inline embedding result exceeds the JSON transport budget; use an out-of-band lease",
        )
    vectors: list[list[float]] = []
    for text in texts:
        if cancelled():
            return
        seed = _stable_digest("embedding", text)
        raw = [((seed[index % len(seed)] / 127.5) - 1.0) for index in range(dimensions)]
        norm = math.sqrt(sum(value * value for value in raw)) or 1.0
        vectors.append([round(value / norm, 8) for value in raw])
    yield "embedding_result", {
        "vectors": vectors,
        "dimensions": dimensions,
        "count": len(vectors),
        "normalized": True,
        "deterministic": True,
    }


def vision(payload: dict[str, Any], cancelled: Cancelled) -> Iterator[Event]:
    digest = validate_text(payload.get("frame_digest"), "frame_digest")
    labels_raw = payload.get("labels", ["face"])
    labels = validate_string_list(labels_raw, "labels", maximum=64)
    if cancelled():
        return
    seed = _stable_digest("vision", digest)
    count = 1 + seed[0] % min(3, len(labels))
    detections = []
    for index in range(count):
        x = round((seed[1 + index] % 50) / 100, 4)
        y = round((seed[5 + index] % 50) / 100, 4)
        width = round(0.2 + (seed[9 + index] % 20) / 100, 4)
        height = round(0.2 + (seed[13 + index] % 20) / 100, 4)
        detections.append(
            {
                "track_fixture_id": f"fixture-{index + 1}",
                "label": labels[index % len(labels)],
                "confidence": round(0.8 + (seed[17 + index] % 20) / 100, 4),
                "box_normalized": {"x": x, "y": y, "width": width, "height": height},
            }
        )
    yield "vision_result", {"frame_digest": digest, "detections": detections, "deterministic": True}


def lip_sync(payload: dict[str, Any], cancelled: Cancelled) -> Iterator[Event]:
    request = MouthResidualRequestV1.parse(payload, allow_fixture_delay=True)
    if cancelled():
        return

    digest_source = "\0".join(
        (
            request.frame_lease_id,
            request.audio_lease_id,
            request.selected_encounter_id,
            request.selected_track_id,
            str(request.track_epoch),
            str(request.source_frame_sequence),
            str(request.source_capture_qpc),
            str(request.cancellation_generation),
            ",".join(
                str(request.mouth_mask_bounds_normalized[field]) for field in ("x", "y", "width", "height")
            ),
        )
    )
    residual_confidence = round(
        min(request.tracking_confidence, 0.8 + (_stable_digest("mouth-residual", digest_source)[0] % 16) / 100),
        4,
    )
    proposal = MouthResidualProposalV1(
        request=request,
        residual_confidence=residual_confidence,
        fail_open_reasons=("metadata_only_stub",),
        deterministic=True,
    )
    yield "mouth_patch_proposal", proposal.to_payload()
    yield "lip_sync_result", {
        "contract_version": MOUTH_RESIDUAL_CONTRACT_VERSION,
        "mode": "external_current_frame_residual_metadata_only",
        "source_frame_sequence": request.source_frame_sequence,
        "track_epoch": request.track_epoch,
        "cancellation_generation": request.cancellation_generation,
        "proposal_presentable": False,
        "fail_open_reason": "metadata_only_stub",
        "no_pixels_inline": True,
        "image_modified": False,
        "full_frame_replacement": False,
        "static_avatar_source": False,
        "experimental": True,
        "deterministic": True,
    }


ENGINES: dict[str, Callable[[dict[str, Any], Cancelled], Iterator[Event]]] = {
    "llm": llm,
    "stt": stt,
    "tts": tts,
    "embedding": embedding,
    "vision": vision,
    "lip_sync": lip_sync,
}
