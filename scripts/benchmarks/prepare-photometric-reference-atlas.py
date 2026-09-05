#!/usr/bin/env python3
"""Prepare a schema-3 photometric full-lip reference atlas.

Every state must be rendered in the same image coordinate system and pose. The
neutral state's mouth-corner midpoint, width, and roll define one shared
canonical sampling map for every state. A single union alpha is likewise shared
by every state, so runtime calibration can compare like-for-like pixels.

The exporter records provenance and coverage, but deliberately never calls a
rendered atlas a qualified teacher. Visual qualification and native loader
admission are separate review steps.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import sys
from typing import Any

import cv2
import numpy as np


STATE_SPEC_SCHEMAS = {
    "interactive-npcs-landmarked-oral-state-specs/v1",
    "interactive-npcs-photometric-reference-state-specs/v1",
}
METADATA_SCHEMA = "interactive-npcs-mediapipe-mouth-landmarks/v1"
QUALITY_SCHEMA = "interactive-npcs-photometric-reference-atlas-quality/v1"
REPRESENTATION = "photometric-full-lip-reference-v1"
NEAR_OPAQUE_ALPHA = 250
MIN_ALPHA_PIXELS = 16
MIN_STATES = 4
MAX_STATES = 16
MAX_CENTER_DRIFT_IN_NEUTRAL_WIDTHS = 0.20
MAX_ROLL_DRIFT_RADIANS = 0.12
MAX_SOURCE_DIMENSION = 8192
MAX_SOURCE_PIXELS = 33_554_432
MAX_METADATA_FRAMES = 4096


def _load_shared_atlas_helpers() -> Any:
    path = Path(__file__).with_name("prepare-landmarked-oral-atlas.py")
    spec = importlib.util.spec_from_file_location("prepare_landmarked_oral_atlas_shared", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load shared atlas helpers: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SHARED = _load_shared_atlas_helpers()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _reject_constant(value: str) -> None:
    raise ValueError(f"non-finite JSON number is not allowed: {value}")


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key is not allowed: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> dict[str, Any]:
    document = json.loads(
        path.read_text(encoding="utf-8"),
        parse_constant=_reject_constant,
        object_pairs_hook=_unique_object,
    )
    if not isinstance(document, dict):
        raise ValueError(f"JSON document must be an object: {path}")
    return document


def finite_unit_number(value: object, field: str) -> float:
    if (
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(float(value))
        or not 0.0 <= float(value) <= 1.0
    ):
        raise ValueError(f"{field} must be a finite number in 0..1")
    return float(value)


def finite_number(value: object, field: str) -> float:
    if (
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(float(value))
    ):
        raise ValueError(f"{field} must be a finite number")
    return float(value)


def basename(value: object, field: str) -> str:
    if not isinstance(value, str) or not value or Path(value).name != value:
        raise ValueError(f"{field} must be a single filename without directories")
    if "/" in value or "\\" in value or value in {".", ".."}:
        raise ValueError(f"{field} must be a single filename without directories")
    return value


def resolved_file(path: Path, field: str) -> Path:
    try:
        resolved = path.resolve(strict=True)
    except OSError as error:
        raise ValueError(f"{field} does not exist: {path}") from error
    if not resolved.is_file():
        raise ValueError(f"{field} must be a file: {resolved}")
    return resolved


def validate_enrollment_binding(value: object) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError("schema-3 photometric atlases require an enrollment binding object")
    allowed = {
        "schemaVersion",
        "gameProfileId",
        "characterId",
        "referenceProvenanceSha256",
        "reviewStatus",
        "reviewEvidenceSha256",
    }
    unknown = sorted(set(value) - allowed)
    if unknown:
        raise ValueError(f"enrollment binding contains unsupported fields: {unknown}")
    if value.get("schemaVersion") != 1:
        raise ValueError("enrollment binding schemaVersion must be 1")
    validated = SHARED.build_enrollment_binding(
        value.get("gameProfileId"),
        value.get("characterId"),
        value.get("referenceProvenanceSha256"),
        value.get("reviewStatus"),
        value.get("reviewEvidenceSha256"),
    )
    if validated is None:
        raise ValueError("schema-3 photometric atlases require an enrollment binding")
    return validated


def resolve_asset(root: Path, name: object, field: str) -> Path:
    safe_name = basename(name, field)
    resolved = resolved_file(root / safe_name, field)
    if resolved.parent != root:
        raise ValueError(f"{field} escapes the state-spec directory")
    return resolved


def parse_point_list(value: object, field: str) -> list[list[float]]:
    if not isinstance(value, list) or len(value) < 3:
        raise ValueError(f"{field} must contain at least three points")
    result: list[list[float]] = []
    for index, point in enumerate(value):
        if not isinstance(point, list) or len(point) < 2:
            raise ValueError(f"{field}[{index}] must contain x and y")
        x = finite_unit_number(point[0], f"{field}[{index}].x")
        y = finite_unit_number(point[1], f"{field}[{index}].y")
        z = finite_number(point[2], f"{field}[{index}].z") if len(point) >= 3 else 0.0
        result.append([x, y, z])
    return result


def angle_distance(left: float, right: float) -> float:
    return abs(math.atan2(math.sin(left - right), math.cos(left - right)))


def parse_frame(frame: object, image_name: str, dimensions: tuple[int, int]) -> dict[str, Any]:
    if not isinstance(frame, dict):
        raise ValueError(f"metadata frame must be an object: {image_name}")
    if frame.get("file") != image_name or frame.get("accepted") is not True:
        raise ValueError(f"metadata frame is not accepted for {image_name}")
    width, height = dimensions
    if frame.get("width") != width or frame.get("height") != height:
        raise ValueError(f"metadata dimensions do not match decoded image: {image_name}")
    upper = parse_point_list(frame.get("outerUpper"), f"{image_name}.outerUpper")
    lower = parse_point_list(frame.get("outerLower"), f"{image_name}.outerLower")
    left = np.asarray(upper[0][:2], dtype=np.float64)
    right = np.asarray(upper[-1][:2], dtype=np.float64)
    if np.linalg.norm(left - np.asarray(lower[0][:2], dtype=np.float64)) > 1.0e-4:
        raise ValueError(f"outer contours disagree at the left corner: {image_name}")
    if np.linalg.norm(right - np.asarray(lower[-1][:2], dtype=np.float64)) > 1.0e-4:
        raise ValueError(f"outer contours disagree at the right corner: {image_name}")
    width_normalized = float(np.linalg.norm(right - left))
    if width_normalized <= 1.0e-6:
        raise ValueError(f"mouth corner width is degenerate: {image_name}")
    roll_normalized = math.atan2(float(right[1] - left[1]), float(right[0] - left[0]))
    if "cornerWidth" in frame and abs(
        finite_number(frame["cornerWidth"], f"{image_name}.cornerWidth") - width_normalized
    ) > 1.0e-4:
        raise ValueError(f"metadata cornerWidth disagrees with outer corners: {image_name}")
    if "rollRadians" in frame and angle_distance(
        finite_number(frame["rollRadians"], f"{image_name}.rollRadians"), roll_normalized
    ) > 1.0e-4:
        raise ValueError(f"metadata rollRadians disagrees with outer corners: {image_name}")
    scale = np.asarray([width, height], dtype=np.float64)
    left_pixels = left * scale
    right_pixels = right * scale
    width_pixels = float(np.linalg.norm(right_pixels - left_pixels))
    roll_pixels = math.atan2(
        float(right_pixels[1] - left_pixels[1]),
        float(right_pixels[0] - left_pixels[0]),
    )
    return {
        **frame,
        "outerUpper": upper,
        "outerLower": lower,
        "_cornerLeftNormalized": left,
        "_cornerRightNormalized": right,
        "_cornerMidpointNormalized": (left + right) * 0.5,
        "_cornerWidthNormalized": width_normalized,
        "_rollRadiansNormalized": roll_normalized,
        "_cornerLeftPixels": left_pixels,
        "_cornerRightPixels": right_pixels,
        "_cornerMidpointPixels": (left_pixels + right_pixels) * 0.5,
        "_cornerWidthPixels": width_pixels,
        "_rollRadiansPixels": roll_pixels,
    }


def load_state_inputs(
    state_specs_path: Path,
    neutral_image_path: Path,
    metadata_path: Path,
) -> tuple[dict[str, Any], list[dict[str, Any]], np.ndarray, dict[str, Any]]:
    specs_path = resolved_file(state_specs_path, "state specs")
    root = specs_path.parent
    neutral_path = resolved_file(neutral_image_path, "neutral image")
    metadata_resolved = resolved_file(metadata_path, "metadata")
    if neutral_path.parent != root or metadata_resolved.parent != root:
        raise ValueError("state specs, neutral image, metadata, and state images must share one directory")

    document = load_json(specs_path)
    if document.get("schema") not in STATE_SPEC_SCHEMAS:
        raise ValueError(f"unsupported state-spec schema: {document.get('schema')}")
    raw_states = document.get("states")
    if not isinstance(raw_states, list) or not MIN_STATES <= len(raw_states) <= MAX_STATES:
        raise ValueError(f"state specification must contain {MIN_STATES}..{MAX_STATES} states")
    scope = document.get("scope", "private-review-only")
    if not isinstance(scope, str) or not scope.strip():
        raise ValueError("state specification scope must be a non-empty string")
    notes = document.get("notes", [])
    if not isinstance(notes, list) or any(
        not isinstance(note, str) or not note.strip() for note in notes
    ):
        raise ValueError("state specification notes must be a list of non-empty strings")

    metadata = load_json(metadata_resolved)
    if metadata.get("schema") != METADATA_SCHEMA:
        raise ValueError(f"unsupported landmark metadata schema: {metadata_resolved}")
    metadata_frames = metadata.get("frames")
    if not isinstance(metadata_frames, list):
        raise ValueError("landmark metadata frames must be a list")
    if len(metadata_frames) > MAX_METADATA_FRAMES:
        raise ValueError(
            f"landmark metadata may contain at most {MAX_METADATA_FRAMES} frames"
        )

    neutral_image = cv2.imread(str(neutral_path), cv2.IMREAD_COLOR)
    if neutral_image is None:
        raise ValueError(f"could not decode neutral image: {neutral_path}")
    dimensions = (int(neutral_image.shape[1]), int(neutral_image.shape[0]))
    if dimensions[0] < 16 or dimensions[1] < 16:
        raise ValueError("reference images must be at least 16 by 16 pixels")
    if (
        dimensions[0] > MAX_SOURCE_DIMENSION
        or dimensions[1] > MAX_SOURCE_DIMENSION
        or dimensions[0] * dimensions[1] > MAX_SOURCE_PIXELS
    ):
        raise ValueError(
            "reference images exceed the 8192-pixel edge or 33,554,432-pixel decode limit"
        )

    states: list[dict[str, Any]] = []
    names: set[str] = set()
    for index, raw_state in enumerate(raw_states):
        if not isinstance(raw_state, dict):
            raise ValueError(f"state {index} must be an object")
        name = raw_state.get("name")
        if not isinstance(name, str) or not name.strip() or name in names:
            raise ValueError("state names must be unique non-empty strings")
        names.add(name)
        image_path = resolve_asset(root, raw_state.get("image"), f"state {name} image")
        if "metadata" in raw_state:
            declared_metadata = resolve_asset(
                root, raw_state["metadata"], f"state {name} metadata"
            )
            if declared_metadata != metadata_resolved:
                raise ValueError(f"state {name} metadata does not match --metadata")
        coefficients = raw_state.get("coefficients")
        if not isinstance(coefficients, list) or len(coefficients) != 8:
            raise ValueError(f"state {name} coefficients must contain eight values")
        coefficients = [
            finite_unit_number(value, f"state {name} coefficients[{coefficient_index}]")
            for coefficient_index, value in enumerate(coefficients)
        ]
        if "transparent" in raw_state and not isinstance(raw_state["transparent"], bool):
            raise ValueError(f"state {name} transparent flag must be boolean")
        for field in ("coverage", "disclosure"):
            if field in raw_state and (
                not isinstance(raw_state[field], str) or not raw_state[field].strip()
            ):
                raise ValueError(f"state {name} {field} must be a non-empty string")
        if "referenceType" in raw_state and raw_state["referenceType"] not in SHARED.REFERENCE_TYPES:
            raise ValueError(
                f"state {name} referenceType must be observed, generated, or geometry-transfer"
            )
        if "articulation" in raw_state and (
            not isinstance(raw_state["articulation"], str)
            or not raw_state["articulation"].strip()
        ):
            raise ValueError(f"state {name} articulation must be a non-empty string")
        image = cv2.imread(str(image_path), cv2.IMREAD_COLOR)
        if image is None:
            raise ValueError(f"could not decode state image: {image_path}")
        if (image.shape[1], image.shape[0]) != dimensions:
            raise ValueError(f"state image dimensions do not match neutral image: {image_path}")
        matches = [frame for frame in metadata_frames if isinstance(frame, dict) and frame.get("file") == image_path.name]
        if len(matches) != 1:
            raise ValueError(f"metadata must contain exactly one frame for {image_path.name}")
        parsed_frame = parse_frame(matches[0], image_path.name, dimensions)
        reference_type, reference_type_basis = SHARED.infer_reference_type(raw_state)
        articulation = SHARED.infer_articulation({**raw_state, "coefficients": coefficients})
        states.append(
            {
                **raw_state,
                "name": name,
                "image": image_path.name,
                "_imagePath": image_path,
                "_image": image,
                "_frame": parsed_frame,
                "coefficients": coefficients,
                "referenceType": reference_type,
                "referenceTypeBasis": reference_type_basis,
                "articulation": articulation,
            }
        )

    neutral_name = neutral_path.name
    neutral_coefficients = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    if states[0]["name"].strip().lower() != "neutral" or states[0]["_imagePath"] != neutral_path:
        raise ValueError("state 0 must be named neutral and reference --neutral-image")
    if states[0]["coefficients"] != neutral_coefficients:
        raise ValueError("state 0 must use the exact neutral-contact coefficient vector")

    neutral_frame = states[0]["_frame"]
    neutral_midpoint = neutral_frame["_cornerMidpointPixels"]
    neutral_width = float(neutral_frame["_cornerWidthPixels"])
    neutral_roll = float(neutral_frame["_rollRadiansPixels"])
    for state in states:
        frame = state["_frame"]
        center_drift = (
            float(np.linalg.norm(frame["_cornerMidpointPixels"] - neutral_midpoint))
            / neutral_width
        )
        roll_drift = angle_distance(float(frame["_rollRadiansPixels"]), neutral_roll)
        if center_drift > MAX_CENTER_DRIFT_IN_NEUTRAL_WIDTHS:
            raise ValueError(
                f"state {state['name']} is not in the shared registration: mouth midpoint drift "
                f"{center_drift:.6f} exceeds {MAX_CENTER_DRIFT_IN_NEUTRAL_WIDTHS:.2f} neutral widths"
            )
        if roll_drift > MAX_ROLL_DRIFT_RADIANS:
            raise ValueError(
                f"state {state['name']} is not in the shared pose: roll drift "
                f"{roll_drift:.6f} exceeds {MAX_ROLL_DRIFT_RADIANS:.2f} radians"
            )
        state["_centerDriftNeutralWidths"] = center_drift
        state["_rollDriftRadians"] = roll_drift

    observed_by_image: dict[str, set[str]] = {}
    for state in states:
        if state["referenceType"] == "observed" and state["articulation"] != "neutral":
            observed_by_image.setdefault(state["image"], set()).add(state["articulation"])
    conflicts = {
        image: sorted(articulations)
        for image, articulations in observed_by_image.items()
        if len(articulations) > 1
    }
    if conflicts:
        raise ValueError(
            "one observed image cannot prove multiple articulations: "
            + ", ".join(f"{image}={values}" for image, values in sorted(conflicts.items()))
        )

    return (
        {
            **document,
            "scope": scope.strip(),
            "_path": specs_path,
            "_metadataPath": metadata_resolved,
            "_neutralPath": neutral_path,
            "_dimensions": dimensions,
        },
        states,
        neutral_image,
        neutral_frame,
    )


def canonical_map(neutral_frame: dict[str, Any], width: int, height: int) -> tuple[np.ndarray, np.ndarray, dict[str, float | list[float]]]:
    image_width = float(neutral_frame["width"])
    image_height = float(neutral_frame["height"])
    left = np.asarray(neutral_frame["outerUpper"][0][:2], dtype=np.float64) * (image_width, image_height)
    right = np.asarray(neutral_frame["outerUpper"][-1][:2], dtype=np.float64) * (image_width, image_height)
    center = (left + right) * 0.5
    neutral_width = float(np.linalg.norm(right - left))
    roll = math.atan2(float(right[1] - left[1]), float(right[0] - left[0]))
    canonical_width = neutral_width * 1.34
    canonical_height = canonical_width * 0.625
    unit_x = (np.arange(width, dtype=np.float32) + 0.5) / width * 2.0 - 1.0
    unit_y = (np.arange(height, dtype=np.float32) + 0.5) / height * 2.0 - 1.0
    grid_x, grid_y = np.meshgrid(unit_x, unit_y)
    local_x = grid_x * canonical_width * 0.5
    local_y = grid_y * canonical_height * 0.5
    cosine, sine = math.cos(roll), math.sin(roll)
    map_x = (center[0] + cosine * local_x - sine * local_y).astype(np.float32)
    map_y = (center[1] + sine * local_x + cosine * local_y).astype(np.float32)
    return map_x, map_y, {
        "cornerMidpointPixels": [float(center[0]), float(center[1])],
        "neutralCornerWidthPixels": neutral_width,
        "rollRadians": roll,
        "canonicalWidthPixels": canonical_width,
        "canonicalHeightPixels": canonical_height,
    }


def outer_polygon(frame: dict[str, Any]) -> np.ndarray:
    width, height = int(frame["width"]), int(frame["height"])
    points = frame["outerUpper"] + list(reversed(frame["outerLower"]))
    # Keep the prototype's deterministic truncation behavior. The pixel-center
    # sampling map and every other floating-point operation are shared as well.
    return np.asarray(
        [[point[0] * width, point[1] * height] for point in points],
        dtype=np.int32,
    )


def build_common_alpha(
    states: list[dict[str, Any]],
    map_x: np.ndarray,
    map_y: np.ndarray,
    width: int,
    height: int,
) -> tuple[np.ndarray, np.ndarray]:
    source_height, source_width = states[0]["_image"].shape[:2]
    source_mask = np.zeros((source_height, source_width), np.uint8)
    for state in states:
        cv2.fillPoly(source_mask, [outer_polygon(state["_frame"])], 255, lineType=cv2.LINE_8)
    source_mask = cv2.dilate(source_mask, np.ones((9, 9), np.uint8), iterations=1)
    source_alpha = cv2.GaussianBlur(source_mask.astype(np.float32) / 255.0, (0, 0), 3.0)
    alpha = cv2.remap(source_alpha, map_x, map_y, cv2.INTER_LINEAR)
    alpha = np.where(alpha < 1.0 / 255.0, 0.0, np.clip(alpha, 0.0, 1.0))
    alpha_byte = np.rint(alpha * 255.0).astype(np.uint8)
    validate_common_alpha(alpha_byte, width, height)
    return alpha, alpha_byte


def validate_common_alpha(alpha: np.ndarray, width: int, height: int) -> dict[str, int]:
    if alpha.dtype != np.uint8 or alpha.shape != (height, width):
        raise ValueError("common alpha has the wrong type or dimensions")
    zero = int(np.count_nonzero(alpha == 0))
    near_opaque = int(np.count_nonzero(alpha >= NEAR_OPAQUE_ALPHA))
    if near_opaque < MIN_ALPHA_PIXELS:
        raise ValueError(
            f"common alpha needs at least {MIN_ALPHA_PIXELS} near-opaque pixels; got {near_opaque}"
        )
    if zero < MIN_ALPHA_PIXELS:
        raise ValueError(
            f"common alpha needs at least {MIN_ALPHA_PIXELS} zero-alpha pixels; got {zero}"
        )
    return {"zeroAlphaPixels": zero, "nearOpaquePixels": near_opaque}


def premultiplied_state(
    image: np.ndarray,
    map_x: np.ndarray,
    map_y: np.ndarray,
    alpha: np.ndarray,
    alpha_byte: np.ndarray,
) -> tuple[bytes, np.ndarray]:
    sampled = cv2.remap(image, map_x, map_y, cv2.INTER_LANCZOS4)
    premultiplied = np.minimum(
        np.rint(sampled.astype(np.float32) * alpha[:, :, None]).astype(np.uint8),
        alpha_byte[:, :, None],
    )
    bgra = np.dstack((premultiplied, alpha_byte))
    return bgra.tobytes(), sampled


def coverage_document(states: list[dict[str, Any]]) -> dict[str, Any]:
    slot_articulations = sorted(
        {
            str(state["articulation"])
            for state in states
            if state["articulation"] not in {"neutral", "unclassified"}
        }
    )
    observed_articulations = sorted(
        {
            str(state["articulation"])
            for state in states
            if state["referenceType"] == "observed"
            and state["articulation"] not in {"neutral", "unclassified"}
        }
    )
    missing = sorted(set(slot_articulations) - set(observed_articulations))
    unclassified = sorted(
        str(state["name"])
        for state in states
        if state["articulation"] == "unclassified"
    )
    return {
        "status": "incomplete" if missing or unclassified else "observed-slots-declared",
        "slotArticulations": slot_articulations,
        "observedArticulations": observed_articulations,
        "missingObservedArticulations": missing,
        "unclassifiedStateNames": unclassified,
        "referenceTypeCounts": {
            reference_type: sum(1 for state in states if state["referenceType"] == reference_type)
            for reference_type in sorted(SHARED.REFERENCE_TYPES)
        },
        "qualificationBoundary": (
            "Coverage labels describe provenance only. They do not qualify visual quality, "
            "phoneme accuracy, temporal behavior, native admission, or distribution."
        ),
    }


def make_board(samples: list[tuple[str, np.ndarray]], alpha: np.ndarray, output: Path) -> None:
    height, width = alpha.shape
    checker = np.repeat(
        np.where(np.indices((height, width)).sum(axis=0) // 8 % 2 == 0, 42, 72)[:, :, None],
        3,
        axis=2,
    ).astype(np.float32)
    scale = alpha.astype(np.float32)[:, :, None]
    rows: list[np.ndarray] = []
    for name, sampled in samples:
        preview = np.rint(sampled.astype(np.float32) * scale + checker * (1.0 - scale)).astype(np.uint8)
        cv2.putText(preview, name, (8, 20), cv2.FONT_HERSHEY_SIMPLEX, 0.54, (255, 255, 255), 2, cv2.LINE_AA)
        cv2.putText(preview, name, (8, 20), cv2.FONT_HERSHEY_SIMPLEX, 0.54, (0, 0, 0), 1, cv2.LINE_AA)
        rows.append(preview)
    if not cv2.imwrite(str(output), np.concatenate(rows, axis=0)):
        raise ValueError(f"failed to write atlas state board: {output}")


def export_atlas(
    state_specs_path: Path,
    neutral_image_path: Path,
    metadata_path: Path,
    output: Path,
    width: int,
    height: int,
    enrollment_binding: dict[str, Any],
) -> dict[str, Any]:
    if not 64 <= width <= 512 or not 64 <= height <= 512:
        raise ValueError("canonical dimensions must be in 64..512")
    enrollment_binding = validate_enrollment_binding(enrollment_binding)
    output = output.resolve(strict=False)
    if output.exists():
        raise ValueError(f"refusing to overwrite existing atlas: {output}")

    document, states, _neutral_image, neutral_frame = load_state_inputs(
        state_specs_path, neutral_image_path, metadata_path
    )
    map_x, map_y, canonical = canonical_map(neutral_frame, width, height)
    alpha, alpha_byte = build_common_alpha(states, map_x, map_y, width, height)
    alpha_metrics = validate_common_alpha(alpha_byte, width, height)
    alpha_hash = sha256_bytes(alpha_byte.tobytes())

    texture = bytearray()
    samples: list[tuple[str, np.ndarray]] = []
    manifest_states: list[dict[str, Any]] = []
    quality_states: list[dict[str, Any]] = []
    image_hashes: dict[str, str] = {}
    for index, state in enumerate(states):
        encoded, sampled = premultiplied_state(
            state["_image"], map_x, map_y, alpha, alpha_byte
        )
        texture.extend(encoded)
        samples.append((state["name"], sampled))
        image_hash = image_hashes.setdefault(state["image"], sha256(state["_imagePath"]))
        manifest_states.append(
            {
                "index": index,
                "coefficients": state["coefficients"],
                "enrolledPose": [0.0, 0.0, 0.0],
            }
        )
        quality_states.append(
            {
                "index": index,
                "name": state["name"],
                "image": state["image"],
                "imageSha256": image_hash,
                "referenceType": state["referenceType"],
                "referenceTypeBasis": state["referenceTypeBasis"],
                "articulation": state["articulation"],
                "coverage": state.get("coverage"),
                "disclosure": state.get("disclosure"),
                "sourceTransparentFlagIgnored": bool(state.get("transparent", False)),
                "centerDriftNeutralWidths": round(state["_centerDriftNeutralWidths"], 8),
                "rollDriftRadians": round(state["_rollDriftRadians"], 8),
            }
        )

    specs_hash = sha256(document["_path"])
    metadata_hash = sha256(document["_metadataPath"])
    neutral_hash = sha256(document["_neutralPath"])
    lineage_payload = {
        "representation": REPRESENTATION,
        "stateSpecsSha256": specs_hash,
        "metadataSha256": metadata_hash,
        "neutralImageSha256": neutral_hash,
        "stateImageSha256": [image_hashes[state["image"]] for state in states],
        "width": width,
        "height": height,
    }
    lineage = sha256_bytes(
        (json.dumps(lineage_payload, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
    )

    output.mkdir(parents=True)
    texture_path = output / "atlas-bgra8-premultiplied.bin"
    texture_path.write_bytes(texture)
    texture_hash = sha256(texture_path)
    state_bytes = width * height * 4
    manifest = {
        "schemaVersion": 3,
        "neutralStateIndex": 0,
        "identityRevision": int(lineage[:16], 16) or 1,
        "texture": {
            "file": texture_path.name,
            "sha256": texture_hash,
            "representation": REPRESENTATION,
            "width": width,
            "height": height,
            "strideBytes": width * 4,
            "stateCount": len(states),
            "stateBytes": state_bytes,
        },
        "states": manifest_states,
        "enrollmentBinding": enrollment_binding,
    }
    manifest_path = output / "atlas.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    coverage = coverage_document(states)
    quality = {
        "schema": QUALITY_SCHEMA,
        "renderingStatus": "rendered",
        "qualityStatus": "rendered-not-qualified",
        "teacherQualified": False,
        "scope": document["scope"],
        "notes": document.get("notes", []),
        "representation": REPRESENTATION,
        "input": {
            "stateSpecs": str(document["_path"]),
            "stateSpecsSha256": specs_hash,
            "neutralImage": str(document["_neutralPath"]),
            "neutralImageSha256": neutral_hash,
            "metadata": str(document["_metadataPath"]),
            "metadataSha256": metadata_hash,
        },
        "sharedRegistration": {
            "required": True,
            "basis": "one neutral corner-midpoint sampling map for all already-aligned states",
            "sourceDimensions": list(document["_dimensions"]),
            "maximumCenterDriftNeutralWidths": MAX_CENTER_DRIFT_IN_NEUTRAL_WIDTHS,
            "maximumRollDriftRadians": MAX_ROLL_DRIFT_RADIANS,
            **canonical,
        },
        "commonAlpha": {
            "sharedByEveryState": True,
            "sha256": alpha_hash,
            "nearOpaqueThreshold": NEAR_OPAQUE_ALPHA,
            **alpha_metrics,
        },
        "coverage": coverage,
        "states": quality_states,
        "enrollmentBinding": {
            "value": enrollment_binding,
            "reviewedBindingDeclared": enrollment_binding["reviewStatus"] == "reviewed-private",
            "loaderAdmissionVerified": False,
        },
        "qualificationBoundary": (
            "Successful export proves deterministic layout, hashes, alpha support, and declared "
            "provenance. It does not prove visual quality, temporal lip sync, game capture behavior, "
            "native loader admission, or distribution readiness."
        ),
    }
    quality_path = output / "atlas-quality.json"
    quality_path.write_text(json.dumps(quality, indent=2) + "\n", encoding="utf-8")
    board_path = output / "atlas-state-board.png"
    make_board(samples, alpha, board_path)

    return {
        "status": "prepared",
        "qualityStatus": quality["qualityStatus"],
        "teacherQualified": False,
        "output": str(output),
        "states": len(states),
        "textureBytes": len(texture),
        "textureSha256": texture_hash,
        "manifestSha256": sha256(manifest_path),
        "commonAlphaSha256": alpha_hash,
        **alpha_metrics,
        "coverageStatus": coverage["status"],
        "quality": str(quality_path),
        "board": str(board_path),
    }


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--state-specs", type=Path, required=True)
    parser.add_argument("--neutral-image", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--width", type=int, default=256)
    parser.add_argument("--height", type=int, default=160)
    parser.add_argument("--game-profile-id", required=True)
    parser.add_argument("--character-id", required=True)
    parser.add_argument(
        "--reference-provenance-sha256",
        action="append",
        required=True,
        help="repeatable lowercase SHA-256 binding the atlas to private reference evidence",
    )
    parser.add_argument("--review-status", choices=("reviewed-private", "unreviewed"), required=True)
    parser.add_argument("--review-evidence-sha256")
    args = parser.parse_args()
    enrollment_binding = SHARED.build_enrollment_binding(
        args.game_profile_id,
        args.character_id,
        args.reference_provenance_sha256,
        args.review_status,
        args.review_evidence_sha256,
    )
    if enrollment_binding is None:
        raise ValueError("schema-3 photometric atlases require an enrollment binding")
    result = export_atlas(
        args.state_specs,
        args.neutral_image,
        args.metadata,
        args.output,
        args.width,
        args.height,
        enrollment_binding,
    )
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"photometric reference atlas error: {error}", file=sys.stderr)
        sys.exit(2)
