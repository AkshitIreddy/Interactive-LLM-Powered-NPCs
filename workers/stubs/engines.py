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
        validate_contained_region,
        validate_generation,
        validate_normalized_region,
        validate_opaque_id,
        validate_positive_int,
        validate_string_list,
        validate_text,
    )
except ImportError:
    from contract import (
        ContractError,
        MAX_EMBEDDING_DIMENSIONS,
        validate_contained_region,
        validate_generation,
        validate_normalized_region,
        validate_opaque_id,
        validate_positive_int,
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
    allowed = {
        "frame_lease_id",
        "audio_lease_id",
        "frame_lease_expires_qpc",
        "audio_lease_expires_qpc",
        "selected_encounter_id",
        "selected_track_id",
        "source_frame_sequence",
        "source_capture_qpc",
        "qpc_frequency_hz",
        "face_region_normalized",
        "mouth_region_normalized",
        "presentation_deadline_qpc",
        "cancellation_generation",
        "fixture_event_delay_ms",
    }
    unknown = sorted(set(payload) - allowed)
    if unknown:
        raise ContractError(
            "invalid_payload",
            "generic lip-sync payload contains unsupported or inline media fields",
            details={"fields": unknown[:16]},
        )

    frame_lease_id = validate_opaque_id(payload.get("frame_lease_id"), "frame_lease_id")
    audio_lease_id = validate_opaque_id(payload.get("audio_lease_id"), "audio_lease_id")
    encounter_id = validate_opaque_id(payload.get("selected_encounter_id"), "selected_encounter_id")
    track_id = validate_opaque_id(payload.get("selected_track_id"), "selected_track_id")
    frame_sequence = validate_positive_int(payload.get("source_frame_sequence"), "source_frame_sequence")
    capture_qpc = validate_positive_int(payload.get("source_capture_qpc"), "source_capture_qpc")
    qpc_frequency = validate_positive_int(payload.get("qpc_frequency_hz"), "qpc_frequency_hz")
    presentation_deadline = validate_positive_int(
        payload.get("presentation_deadline_qpc"), "presentation_deadline_qpc"
    )
    frame_expiry = validate_positive_int(payload.get("frame_lease_expires_qpc"), "frame_lease_expires_qpc")
    audio_expiry = validate_positive_int(payload.get("audio_lease_expires_qpc"), "audio_lease_expires_qpc")
    generation = validate_generation(payload.get("cancellation_generation"))
    face = validate_normalized_region(payload.get("face_region_normalized"), "face_region_normalized")
    mouth = validate_normalized_region(payload.get("mouth_region_normalized"), "mouth_region_normalized")
    validate_contained_region(mouth, face, "mouth_region_normalized", "face_region_normalized")

    if presentation_deadline <= capture_qpc:
        raise ContractError("stale_source_frame", "presentation deadline must follow source capture time")
    if frame_expiry < presentation_deadline or audio_expiry < presentation_deadline:
        raise ContractError("stale_media_lease", "frame and audio leases must remain valid through presentation")
    if cancelled():
        return

    digest_source = "\0".join(
        (
            frame_lease_id,
            audio_lease_id,
            encounter_id,
            track_id,
            str(frame_sequence),
            str(capture_qpc),
            str(generation),
            ",".join(str(mouth[field]) for field in ("x", "y", "width", "height")),
        )
    )
    confidence = round(0.8 + (_stable_digest("mouth-patch", digest_source)[0] % 16) / 100, 4)
    yield "mouth_patch_proposal", {
        "proposal_kind": "external_current_frame_mouth_residual",
        "frame_lease_id": frame_lease_id,
        "audio_lease_id": audio_lease_id,
        "selected_encounter_id": encounter_id,
        "selected_track_id": track_id,
        "source_frame_sequence": frame_sequence,
        "source_capture_qpc": capture_qpc,
        "qpc_frequency_hz": qpc_frequency,
        "cancellation_generation": generation,
        "confidence": confidence,
        "mask_bounds_normalized": mouth,
        "freshness": {
            "presentation_deadline_qpc": presentation_deadline,
            "frame_lease_expires_qpc": frame_expiry,
            "audio_lease_expires_qpc": audio_expiry,
            "requires_exact_source_frame_sequence": True,
            "discard_if_source_advanced": True,
            "discard_if_generation_changed": True,
            "restore_unmodified_on_rejection": True,
        },
        "patch_lease_id": None,
        "presentable": False,
        "metadata_only": True,
        "no_pixels_inline": True,
        "image_modified": False,
        "deterministic": True,
    }
    yield "lip_sync_result", {
        "mode": "external_current_frame_metadata_only",
        "source_frame_sequence": frame_sequence,
        "cancellation_generation": generation,
        "proposal_presentable": False,
        "no_pixels_inline": True,
        "image_modified": False,
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
