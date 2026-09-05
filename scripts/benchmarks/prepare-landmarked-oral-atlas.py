#!/usr/bin/env python3
"""Export a private native oral atlas from landmarked identity observations.

The atlas deliberately stores only observed oral-interior pixels.  Current
frame lip surfaces remain under native contour-warp ownership, which avoids
replacing sharp game pixels with a rectangular or elliptical generated face
patch.  Dense inner-mouth polygons provide the enrollment boundary and each
state is normalized to the fixed canonical oral slot consumed by the native
compositor.

Inputs are private review observations under E:\\temp.  This script downloads
nothing and never treats generated images as a distributable model pack.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import sys

import cv2
import numpy as np


DEFAULT_STATE_SPECS = (
    ("neutral", "mara-neutral.png", "mara-neutral-mediapipe.json", (0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)),
    ("ah-open", "mara-ah-open.png", "mara-ah-open-mediapipe.json", (0.96, 0.0, 0.0, 0.0, 0.04, 0.04, 0.18, 0.90)),
    ("ee-spread", "mara-ee-spread.png", "mara-ee-spread-mediapipe.json", (0.43, 0.0, 0.0, 0.0, 0.90, 0.90, 0.38, 0.42)),
    ("fv-labiodental", "mara-fv-labiodental.png", "mara-fv-labiodental-mediapipe.json", (0.18, 0.52, 0.08, 0.02, 0.18, 0.18, 0.72, 0.18)),
    # The first OO observation is genuinely near-contact and is labelled as
    # such so nearest-state selection cannot use it for ordinary audible O/U.
    ("oo-rounded-contact", "mara-oo-rounded.png", "mara-oo-rounded-mediapipe.json", (0.08, 0.86, 0.88, 0.92, 0.0, 0.0, 0.08, 0.08)),
    # The nominal OO-open observation contains only a few soft interior rows.
    # Reuse the same actor's sharp AH oral anatomy for the audible rounded slot;
    # runtime contour geometry supplies the O outline while this state supplies
    # source-coherent teeth, tongue and cavity detail.
    ("oo-rounded-open-anatomy-transfer", "mara-ah-open.png", "mara-ah-open-mediapipe.json", (0.38, 0.0, 0.82, 0.76, 0.0, 0.0, 0.08, 0.38)),
)

STATE_SPEC_SCHEMA = "interactive-npcs-landmarked-oral-state-specs/v1"


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def require_e_temp(path: Path, *, must_exist: bool = False) -> Path:
    resolved = path.resolve(strict=must_exist)
    if resolved.drive.lower() != "e:" or "temp" not in {
        part.lower() for part in resolved.parts
    }:
        raise ValueError("private atlas inputs and outputs must stay under E:\\temp")
    return resolved


def parse_observation(root: Path, image_name: str, metadata_name: str) -> tuple[np.ndarray, dict]:
    image_path = root / image_name
    metadata_path = root / metadata_name
    image = cv2.imread(str(image_path), cv2.IMREAD_COLOR)
    if image is None:
        raise ValueError(f"could not decode observation: {image_path}")
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    if metadata.get("schema") != "interactive-npcs-mediapipe-mouth-landmarks/v1":
        raise ValueError(f"unsupported landmark schema: {metadata_path}")
    frames = [
        frame for frame in metadata.get("frames", [])
        if frame.get("file") == image_name
    ]
    if len(frames) != 1 or not frames[0].get("accepted"):
        raise ValueError(f"observation is not uniquely accepted: {metadata_path}")
    frame = frames[0]
    if frame.get("file") != image_name:
        raise ValueError(f"landmark image mismatch: {metadata_path}")
    if [image.shape[1], image.shape[0]] != [int(frame["width"]), int(frame["height"])]:
        raise ValueError(f"observation dimensions changed: {image_path}")
    return image, frame


def load_state_specs(path: Path | None) -> tuple[list[dict[str, object]], str, dict[str, object]]:
    if path is None:
        states = [
            {
                "name": name,
                "image": image,
                "metadata": metadata,
                "coefficients": list(coefficients),
            }
            for name, image, metadata, coefficients in DEFAULT_STATE_SPECS
        ]
        return states, "private-synthetic-mara-review-only", {}
    resolved = require_e_temp(path, must_exist=True)
    document = json.loads(resolved.read_text(encoding="utf-8"))
    if document.get("schema") != STATE_SPEC_SCHEMA:
        raise ValueError(f"unsupported state-spec schema: {resolved}")
    states = document.get("states")
    if not isinstance(states, list) or not 4 <= len(states) <= 64:
        raise ValueError("state specification must contain 4..64 states")
    names: set[str] = set()
    for state in states:
        if not isinstance(state, dict):
            raise ValueError("each state specification must be an object")
        name = state.get("name")
        image = state.get("image")
        metadata = state.get("metadata")
        coefficients = state.get("coefficients")
        if not isinstance(name, str) or not name or name in names:
            raise ValueError("state names must be unique non-empty strings")
        names.add(name)
        if not isinstance(image, str) or Path(image).name != image:
            raise ValueError(f"state image must be a basename: {name}")
        if not isinstance(metadata, str) or Path(metadata).name != metadata:
            raise ValueError(f"state metadata must be a basename: {name}")
        if (
            not isinstance(coefficients, list)
            or len(coefficients) != 8
            or any(
                not isinstance(value, (int, float))
                or not math.isfinite(float(value))
                or not 0.0 <= float(value) <= 1.0
                for value in coefficients
            )
        ):
            raise ValueError(f"state coefficients must contain eight finite unit values: {name}")
        if "transparent" in state and not isinstance(state["transparent"], bool):
            raise ValueError(f"state transparent flag must be boolean: {name}")
        if "sourceSeconds" in state and (
            not isinstance(state["sourceSeconds"], (int, float))
            or not math.isfinite(float(state["sourceSeconds"]))
            or float(state["sourceSeconds"]) < 0.0
        ):
            raise ValueError(f"state sourceSeconds must be a finite non-negative number: {name}")
        for field in ("coverage", "disclosure"):
            if field in state and (
                not isinstance(state[field], str) or not state[field]
            ):
                raise ValueError(f"state {field} must be a non-empty string: {name}")
    scope = document.get("scope", "private-review-only")
    if not isinstance(scope, str) or not scope:
        raise ValueError("state specification scope must be a non-empty string")
    return states, scope, {
        "file": str(resolved),
        "sha256": sha256(resolved),
        "notes": document.get("notes", []),
    }


def canonical_map(frame: dict, width: int, height: int) -> tuple[np.ndarray, np.ndarray]:
    image_width = float(frame["width"])
    image_height = float(frame["height"])
    center_x = float(frame["center"][0]) * image_width
    center_y = float(frame["center"][1]) * image_height
    mouth_width = float(frame["cornerWidth"]) * image_width
    roll = float(frame["rollRadians"])
    canonical_width = mouth_width * 1.34
    canonical_height = canonical_width * 0.625
    unit_x = (np.arange(width, dtype=np.float32) + 0.5) / width * 2.0 - 1.0
    unit_y = (np.arange(height, dtype=np.float32) + 0.5) / height * 2.0 - 1.0
    grid_x, grid_y = np.meshgrid(unit_x, unit_y)
    local_x = grid_x * canonical_width * 0.5
    local_y = grid_y * canonical_height * 0.5
    cosine, sine = math.cos(roll), math.sin(roll)
    map_x = (center_x + cosine * local_x - sine * local_y).astype(np.float32)
    map_y = (center_y + sine * local_x + cosine * local_y).astype(np.float32)
    return map_x, map_y


def image_point_to_canonical(point: list[float], frame: dict) -> tuple[float, float]:
    image_width = float(frame["width"])
    image_height = float(frame["height"])
    center_x = float(frame["center"][0]) * image_width
    center_y = float(frame["center"][1]) * image_height
    mouth_width = float(frame["cornerWidth"]) * image_width
    canonical_width = mouth_width * 1.34
    canonical_height = canonical_width * 0.625
    roll = float(frame["rollRadians"])
    delta_x = float(point[0]) * image_width - center_x
    delta_y = float(point[1]) * image_height - center_y
    cosine, sine = math.cos(roll), math.sin(roll)
    local_x = cosine * delta_x + sine * delta_y
    local_y = -sine * delta_x + cosine * delta_y
    return local_x / (canonical_width * 0.5), local_y / (canonical_height * 0.5)


def canonical_to_pixel(point: tuple[float, float], width: int, height: int) -> tuple[int, int]:
    x = int(round((point[0] + 1.0) * 0.5 * width - 0.5))
    y = int(round((point[1] + 1.0) * 0.5 * height - 0.5))
    return min(width - 1, max(0, x)), min(height - 1, max(0, y))


def inner_polygon(frame: dict, width: int, height: int) -> np.ndarray:
    upper = frame["innerUpper"]
    lower = frame["innerLower"]
    # Both lists include the same left/right corners.  Walk the upper contour
    # left-to-right, then lower right-to-left without duplicating corners.
    ordered = upper + list(reversed(lower[1:-1]))
    return np.asarray(
        [canonical_to_pixel(image_point_to_canonical(point, frame), width, height) for point in ordered],
        dtype=np.int32,
    )


def sharpen_observation(image: np.ndarray) -> np.ndarray:
    value = image.astype(np.float32)
    fine = value - cv2.GaussianBlur(value, (0, 0), 0.68)
    medium = value - cv2.GaussianBlur(value, (0, 0), 1.45)
    return np.clip(value + fine * 0.56 + medium * 0.12, 0.0, 255.0).astype(np.uint8)


def normalized_oral_state(
    image: np.ndarray,
    frame: dict,
    width: int,
    height: int,
    state_name: str,
    transparent: bool = False,
) -> tuple[np.ndarray, np.ndarray, dict[str, object]]:
    map_x, map_y = canonical_map(frame, width, height)
    canonical = cv2.remap(
        sharpen_observation(image),
        map_x,
        map_y,
        cv2.INTER_LANCZOS4,
        borderMode=cv2.BORDER_REFLECT_101,
    )
    polygon = inner_polygon(frame, width, height)
    source_mask = np.zeros((height, width), np.uint8)
    if not transparent and state_name != "neutral":
        cv2.fillPoly(source_mask, [polygon], 255, lineType=cv2.LINE_AA)
        source_mask = cv2.dilate(
            source_mask,
            cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (5, 3)),
            iterations=1,
        )
    ys, xs = np.nonzero(source_mask)
    if transparent or state_name == "neutral" or xs.size < 8:
        return (
            np.zeros_like(canonical),
            np.zeros((height, width), np.float32),
            {
                "sourceOralBounds": None,
                "sourceOralPixelCount": 0,
                "normalizedAlphaPixelCount": 0,
                "laplacianVariance": 0.0,
            },
        )

    x0 = max(0, int(xs.min()) - 1)
    x1 = min(width, int(xs.max()) + 2)
    y0 = max(0, int(ys.min()) - 1)
    y1 = min(height, int(ys.max()) + 2)
    crop = canonical[y0:y1, x0:x1]
    crop_mask = source_mask[y0:y1, x0:x1]

    unit_x = (np.arange(width, dtype=np.float32) + 0.5) / width * 2.0 - 1.0
    unit_y = (np.arange(height, dtype=np.float32) + 0.5) / height * 2.0 - 1.0
    grid_x, grid_y = np.meshgrid(unit_x, unit_y)
    target_support = (
        (grid_x >= -0.67)
        & (grid_x <= 0.67)
        & (grid_y >= -0.24)
        & (grid_y <= 0.20)
    )
    target_ys, target_xs = np.nonzero(target_support)
    tx0, tx1 = int(target_xs.min()), int(target_xs.max()) + 1
    ty0, ty1 = int(target_ys.min()), int(target_ys.max()) + 1
    target_width, target_height = tx1 - tx0, ty1 - ty0
    resized = cv2.resize(crop, (target_width, target_height), interpolation=cv2.INTER_LANCZOS4)
    resized_mask = cv2.resize(
        crop_mask, (target_width, target_height), interpolation=cv2.INTER_LINEAR
    ).astype(np.float32) / 255.0
    resized_mask = cv2.GaussianBlur(resized_mask, (0, 0), 0.58)
    resized_mask = np.clip(resized_mask, 0.0, 1.0)

    output = np.zeros_like(canonical)
    alpha = np.zeros((height, width), np.float32)
    output[ty0:ty1, tx0:tx1] = resized
    alpha[ty0:ty1, tx0:tx1] = resized_mask
    gray = cv2.cvtColor(resized, cv2.COLOR_BGR2GRAY)
    supported = resized_mask > 0.5
    laplacian = cv2.Laplacian(gray, cv2.CV_32F)
    sharpness = float(np.var(laplacian[supported])) if np.any(supported) else 0.0
    return output, alpha, {
        "sourceOralBounds": [x0, y0, x1, y1],
        "sourceOralPixelCount": int(np.count_nonzero(source_mask)),
        "normalizedAlphaPixelCount": int(np.count_nonzero(alpha > 1.0 / 255.0)),
        "laplacianVariance": round(sharpness, 4),
    }


def premultiply(image: np.ndarray, alpha: np.ndarray) -> bytes:
    alpha = np.clip(alpha, 0.0, 1.0)
    premultiplied = np.rint(image.astype(np.float32) * alpha[..., None]).astype(np.uint8)
    alpha_byte = np.rint(alpha * 255.0).astype(np.uint8)
    return np.dstack((premultiplied, alpha_byte)).tobytes()


def make_board(states: list[tuple[str, np.ndarray, np.ndarray]], output: Path) -> None:
    if not states:
        return
    height, width = states[0][1].shape[:2]
    scale = 2
    board = np.full((height * scale * len(states), width * scale * 2, 3), 16, np.uint8)
    checker = np.indices((height, width)).sum(axis=0) // 8 % 2
    checker = np.where(checker[..., None] == 0, 42, 72).astype(np.uint8)
    checker = np.repeat(checker, 3, axis=2)
    for row, (name, image, alpha) in enumerate(states):
        composite = np.rint(
            image.astype(np.float32) * alpha[..., None]
            + checker.astype(np.float32) * (1.0 - alpha[..., None])
        ).astype(np.uint8)
        mask_view = cv2.cvtColor(np.rint(alpha * 255.0).astype(np.uint8), cv2.COLOR_GRAY2BGR)
        for column, value in enumerate((composite, mask_view)):
            enlarged = cv2.resize(
                value, (width * scale, height * scale), interpolation=cv2.INTER_NEAREST
            )
            y0 = row * height * scale
            x0 = column * width * scale
            board[y0 : y0 + height * scale, x0 : x0 + width * scale] = enlarged
        cv2.putText(
            board,
            name,
            (6, row * height * scale + 20),
            cv2.FONT_HERSHEY_SIMPLEX,
            0.55,
            (245, 245, 245),
            1,
            cv2.LINE_AA,
        )
    cv2.imwrite(str(output), board)


def make_reference_contact_sheet(
    states: list[tuple[str, np.ndarray, dict]], output: Path
) -> None:
    if not states:
        return
    cell_width, cell_height = 520, 250
    board = np.full((cell_height * len(states), cell_width * 2, 3), 16, np.uint8)
    for row, (name, image, frame) in enumerate(states):
        image_height, image_width = image.shape[:2]
        center_x, center_y = (float(value) for value in frame["center"])
        mouth_width = float(frame["cornerWidth"])
        for column, scale in enumerate((5.6, 2.45)):
            normalized_width = max(0.12 if column else 0.25, mouth_width * scale)
            normalized_height = (
                normalized_width
                * image_width
                / image_height
                * cell_height
                / cell_width
            )
            x0 = max(0, int((center_x - normalized_width * 0.5) * image_width))
            x1 = min(image_width, int((center_x + normalized_width * 0.5) * image_width))
            vertical_center = center_y - normalized_height * (0.15 if column == 0 else 0.0)
            y0 = max(0, int((vertical_center - normalized_height * 0.5) * image_height))
            y1 = min(image_height, int((vertical_center + normalized_height * 0.5) * image_height))
            crop = image[y0:y1, x0:x1]
            if crop.size == 0:
                raise ValueError(f"empty reference crop for state: {name}")
            interpolation = cv2.INTER_AREA if crop.shape[1] > cell_width else cv2.INTER_LANCZOS4
            view = cv2.resize(crop, (cell_width, cell_height), interpolation=interpolation)
            destination_y = row * cell_height
            destination_x = column * cell_width
            board[
                destination_y : destination_y + cell_height,
                destination_x : destination_x + cell_width,
            ] = view
        cv2.rectangle(
            board,
            (0, row * cell_height),
            (cell_width * 2, row * cell_height + 28),
            (8, 8, 8),
            -1,
        )
        cv2.putText(
            board,
            name,
            (8, row * cell_height + 20),
            cv2.FONT_HERSHEY_SIMPLEX,
            0.56,
            (245, 245, 245),
            1,
            cv2.LINE_AA,
        )
    if not cv2.imwrite(str(output), board):
        raise ValueError(f"failed to write reference contact sheet: {output}")


def main() -> int:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--observations", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--state-specs",
        type=Path,
        help="optional private JSON state specification; omission preserves the Mara review recipe",
    )
    parser.add_argument("--width", type=int, default=256)
    parser.add_argument("--height", type=int, default=160)
    args = parser.parse_args()
    observations = require_e_temp(args.observations, must_exist=True)
    output = require_e_temp(args.output)
    if output.exists():
        raise ValueError(f"refusing to overwrite existing atlas: {output}")
    if not 64 <= args.width <= 512 or not 64 <= args.height <= 512:
        raise ValueError("canonical dimensions must be in 64..512")
    state_specs, scope, configuration = load_state_specs(args.state_specs)

    texture = bytearray()
    manifest_states: list[dict[str, object]] = []
    quality_states: list[dict[str, object]] = []
    board_states: list[tuple[str, np.ndarray, np.ndarray]] = []
    reference_states: list[tuple[str, np.ndarray, dict]] = []
    lineage_parts: list[str] = []
    for index, state in enumerate(state_specs):
        name = str(state["name"])
        image_name = str(state["image"])
        metadata_name = str(state["metadata"])
        coefficients = tuple(float(value) for value in state["coefficients"])
        image, frame = parse_observation(observations, image_name, metadata_name)
        state_image, alpha, metrics = normalized_oral_state(
            image,
            frame,
            args.width,
            args.height,
            name,
            bool(state.get("transparent", False)),
        )
        texture.extend(premultiply(state_image, alpha))
        image_hash = sha256(observations / image_name)
        metadata_hash = sha256(observations / metadata_name)
        lineage_parts.extend((image_hash, metadata_hash))
        manifest_states.append(
            {
                "index": index,
                "coefficients": list(coefficients),
                "enrolledPose": [0.0, 0.0, math.degrees(float(frame["rollRadians"]))],
            }
        )
        quality_states.append(
            {
                "index": index,
                "name": name,
                "image": image_name,
                "imageSha256": image_hash,
                "metadata": metadata_name,
                "metadataSha256": metadata_hash,
                **{
                    key: state[key]
                    for key in ("sourceSeconds", "coverage", "disclosure")
                    if key in state
                },
                **metrics,
            }
        )
        board_states.append((name, state_image, alpha))
        reference_states.append((name, image, frame))

    output.mkdir(parents=True)
    texture_path = output / "atlas-bgra8-premultiplied.bin"
    texture_path.write_bytes(texture)
    if configuration:
        lineage_parts.append(str(configuration["sha256"]))
    lineage = sha256_bytes(("\n".join(lineage_parts) + "\nlandmarked-oral-v1").encode())
    state_bytes = args.width * args.height * 4
    manifest = {
        "schemaVersion": 2,
        "identityRevision": int(lineage[:16], 16) or 1,
        "texture": {
            "file": texture_path.name,
            "sha256": sha256(texture_path),
            "representation": "normalized-oral-interior-v1",
            "width": args.width,
            "height": args.height,
            "strideBytes": args.width * 4,
            "stateCount": len(manifest_states),
            "stateBytes": state_bytes,
        },
        "states": manifest_states,
    }
    manifest_path = output / "atlas.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    quality_path = output / "atlas-quality.json"
    quality_path.write_text(
        json.dumps(
            {
                "schema": "interactive-npcs-landmarked-oral-atlas-quality/v1",
                "scope": scope,
                "source": str(observations),
                "configuration": configuration or None,
                "textureOwnership": "observed-oral-interior-only",
                "currentFrameOwnership": "lip-surface-and-all-exterior-pixels",
                "states": quality_states,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    board_path = output / "atlas-oral-state-board.png"
    make_board(board_states, board_path)
    reference_board_path = output / "atlas-reference-contact-sheet.png"
    make_reference_contact_sheet(reference_states, reference_board_path)
    print(
        json.dumps(
            {
                "status": "prepared",
                "scope": scope,
                "output": str(output),
                "states": len(manifest_states),
                "textureBytes": len(texture),
                "textureSha256": sha256(texture_path),
                "manifestSha256": sha256(manifest_path),
                "quality": str(quality_path),
                "board": str(board_path),
                "references": str(reference_board_path),
            }
        )
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"landmarked oral atlas error: {error}", file=sys.stderr)
        sys.exit(2)
