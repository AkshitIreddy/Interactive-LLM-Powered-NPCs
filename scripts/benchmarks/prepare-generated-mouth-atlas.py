#!/usr/bin/env python3
"""Build a tiny native review atlas from same-identity mouth references.

This is an enrollment tool, never a gameplay hot-path component.  A user- or
teacher-generated neutral/open/rounded/spread/labiodental reference is reduced
to eight canonical BGRA states consumed by the native mouth worker.  Only a
tight, landmark-normalized mouth crop is retained; full reference images never
enter the application bundle.

All inputs and outputs are intentionally restricted to ``E:\\temp`` so model
and enrollment artifacts cannot consume the space-constrained system drive.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from dataclasses import dataclass
from pathlib import Path
import sys

import cv2
import numpy as np


LANDMARK_SCHEMA = "interactive-npcs-mediapipe-mouth-landmarks/v1"

# Native MouthCoefficients order:
# jaw, close, funnel, pucker, smile-left, smile-right, upper-raise, lower-depress.
STATE_LAYOUT = (
    ("neutral", (0.00, 1.00, 0.00, 0.00, 0.00, 0.00, 0.00, 0.00)),
    ("labiodental", (0.00, 0.48, 0.00, 0.00, 0.00, 0.00, 0.00, 0.18)),
    ("rounded", (0.38, 0.00, 0.82, 0.76, 0.00, 0.00, 0.00, 0.00)),
    ("labiodental", (0.24, 0.00, 0.00, 0.00, 0.00, 0.00, 0.28, 0.00)),
    ("open", (0.92, 0.00, 0.00, 0.00, 0.00, 0.00, 0.32, 0.68)),
    ("spread", (0.32, 0.00, 0.00, 0.00, 0.12, 0.12, 0.00, 0.00)),
    ("spread", (0.52, 0.00, 0.00, 0.00, 0.76, 0.76, 0.00, 0.00)),
    ("rounded", (0.36, 0.00, 0.30, 0.00, 0.00, 0.00, 0.00, 0.00)),
)

# Every enrolled reference has a different raw oral aperture.  Normalize that
# aperture once during enrollment so the runtime can map the current tracked
# opening to one stable source rectangle without retaining landmark metadata or
# running a face model during speech.
ORAL_CANONICAL_BOUNDS = (-0.67, -0.32, 0.67, 0.18)


@dataclass(frozen=True)
class Reference:
    label: str
    image_path: Path
    landmark_path: Path
    image: np.ndarray
    record: dict


def require_e_temp(path: Path, *, must_exist: bool) -> Path:
    resolved = path.resolve(strict=must_exist)
    normalized = str(resolved).replace("/", "\\").lower()
    if resolved.drive.lower() != "e:" or not normalized.startswith("e:\\temp\\"):
        raise ValueError("mouth atlas inputs and outputs must stay below E:\\temp")
    return resolved


def sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def load_reference(label: str, image_path: Path, landmark_path: Path) -> Reference:
    image_path = require_e_temp(image_path, must_exist=True)
    landmark_path = require_e_temp(landmark_path, must_exist=True)
    if not image_path.is_file() or image_path.is_symlink():
        raise ValueError(f"{label} image must be a regular non-symlink file")
    if not landmark_path.is_file() or landmark_path.is_symlink():
        raise ValueError(f"{label} landmarks must be a regular non-symlink file")
    document = json.loads(landmark_path.read_text(encoding="utf-8"))
    if document.get("schema") != LANDMARK_SCHEMA:
        raise ValueError(f"{label} landmarks use an unsupported schema")
    accepted = [
        frame
        for frame in document.get("frames", [])
        if frame.get("accepted") and frame.get("file") == image_path.name
    ]
    if len(accepted) != 1:
        raise ValueError(f"{label} requires exactly one accepted matching landmark record")
    image = cv2.imread(str(image_path), cv2.IMREAD_COLOR)
    if image is None:
        raise ValueError(f"cannot decode {label} image")
    record = accepted[0]
    if int(record.get("width", 0)) != image.shape[1] or int(record.get("height", 0)) != image.shape[0]:
        raise ValueError(f"{label} landmark dimensions do not match the image")
    corner_width = float(record.get("cornerWidth", 0.0)) * image.shape[1]
    if not math.isfinite(corner_width) or corner_width < 16.0:
        raise ValueError(f"{label} mouth is too small for canonical extraction")
    return Reference(label, image_path, landmark_path, image, record)


def canonical_patch(reference: Reference, width: int, height: int) -> np.ndarray:
    record = reference.record
    image_height, image_width = reference.image.shape[:2]
    center_x = float(record["center"][0]) * image_width
    center_y = float(record["center"][1]) * image_height
    corner_width = float(record["cornerWidth"]) * image_width
    roll = float(record["rollRadians"])
    if not all(math.isfinite(value) for value in (center_x, center_y, corner_width, roll)):
        raise ValueError(f"{reference.label} contains non-finite mouth geometry")

    crop_width = corner_width * 1.34
    crop_height = crop_width * 0.625
    x = (np.arange(width, dtype=np.float32) + 0.5) / width * 2.0 - 1.0
    y = (np.arange(height, dtype=np.float32) + 0.5) / height * 2.0 - 1.0
    nx, ny = np.meshgrid(x, y)
    local_x = nx * crop_width * 0.5
    local_y = ny * crop_height * 0.5
    cosine = math.cos(roll)
    sine = math.sin(roll)
    sample_x = center_x + cosine * local_x - sine * local_y
    sample_y = center_y + sine * local_x + cosine * local_y
    sampled = cv2.remap(
        reference.image,
        sample_x.astype(np.float32),
        sample_y.astype(np.float32),
        interpolation=cv2.INTER_LINEAR,
        borderMode=cv2.BORDER_CONSTANT,
        borderValue=(0, 0, 0),
    ).astype(np.float32)

    support = np.power(np.abs(nx), 3.4) + np.power(np.abs(ny), 2.8)
    alpha = np.clip((1.0 - support) / 0.22, 0.0, 1.0)
    alpha = alpha * alpha * (3.0 - 2.0 * alpha)
    inside_source = (
        (sample_x >= 0.0)
        & (sample_y >= 0.0)
        & (sample_x <= image_width - 1.0)
        & (sample_y <= image_height - 1.0)
    )
    alpha *= inside_source.astype(np.float32)
    premultiplied = np.rint(sampled * alpha[..., None]).clip(0, 255).astype(np.uint8)
    alpha_bytes = np.rint(alpha * 255.0).clip(0, 255).astype(np.uint8)
    patch = np.dstack((premultiplied, alpha_bytes))
    if np.any(patch[..., :3] > patch[..., 3, None]):
        raise ValueError(f"{reference.label} extraction is not premultiplied")
    return patch


def normalized_oral_patch(reference: Reference, width: int, height: int) -> np.ndarray:
    """Canonicalize the photographed oral aperture to one fixed source box."""

    patch = canonical_patch(reference, width, height)
    record = reference.record
    image_height, image_width = reference.image.shape[:2]
    center_x = float(record["center"][0]) * image_width
    center_y = float(record["center"][1]) * image_height
    corner_width = float(record["cornerWidth"]) * image_width
    roll = float(record["rollRadians"])
    cosine = math.cos(roll)
    sine = math.sin(roll)
    crop_width = corner_width * 1.34
    crop_height = crop_width * 0.625
    points = np.asarray(
        [*record["innerUpper"], *record["innerLower"]], dtype=np.float64
    )[:, :2]
    delta_x = points[:, 0] * image_width - center_x
    delta_y = points[:, 1] * image_height - center_y
    source_x = (cosine * delta_x + sine * delta_y) / (crop_width * 0.5)
    source_y = (-sine * delta_x + cosine * delta_y) / (crop_height * 0.5)
    source_left = float(np.min(source_x))
    source_right = float(np.max(source_x))
    source_top = float(np.min(source_y))
    source_bottom = float(np.max(source_y))
    if source_right - source_left < 0.20 or source_bottom - source_top < 0.015:
        raise ValueError(f"{reference.label} oral aperture geometry is degenerate")

    destination_left, destination_top, destination_right, destination_bottom = (
        ORAL_CANONICAL_BOUNDS
    )
    destination_x = (np.arange(width, dtype=np.float32) + 0.5) / width * 2.0 - 1.0
    destination_y = (np.arange(height, dtype=np.float32) + 0.5) / height * 2.0 - 1.0
    nx, ny = np.meshgrid(destination_x, destination_y)
    u = (nx - destination_left) / (destination_right - destination_left)
    v = (ny - destination_top) / (destination_bottom - destination_top)
    mapped_x = source_left + u * (source_right - source_left)
    mapped_y = source_top + v * (source_bottom - source_top)
    map_x = ((mapped_x + 1.0) * 0.5 * width - 0.5).astype(np.float32)
    map_y = ((mapped_y + 1.0) * 0.5 * height - 0.5).astype(np.float32)
    remapped = cv2.remap(
        patch,
        map_x,
        map_y,
        interpolation=cv2.INTER_LANCZOS4,
        borderMode=cv2.BORDER_REFLECT_101,
    )
    inside = (
        (nx >= destination_left)
        & (nx <= destination_right)
        & (ny >= destination_top)
        & (ny <= destination_bottom)
    )
    result = patch.copy()
    result[inside] = remapped[inside]
    # Lanczos can overshoot premultiplied channels by one or two values.
    result[..., :3] = np.minimum(result[..., :3], result[..., 3, None])
    return result


def aperture(record: dict) -> float:
    width = float(record["width"])
    height = float(record["height"])
    upper = np.asarray(record["innerUpper"], dtype=np.float64)
    lower = np.asarray(record["innerLower"], dtype=np.float64)
    opening = np.mean((lower[2:9, 1] - upper[2:9, 1]) * height)
    return max(0.0, float(opening) / max(1.0, float(record["cornerWidth"]) * width))


def main() -> int:
    parser = argparse.ArgumentParser()
    for label in ("neutral", "open", "rounded", "spread", "labiodental"):
        parser.add_argument(f"--{label}-image", required=True, type=Path)
        parser.add_argument(f"--{label}-landmarks", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--width", type=int, default=206)
    parser.add_argument("--height", type=int, default=143)
    args = parser.parse_args()

    if not 16 <= args.width <= 512 or not 16 <= args.height <= 512:
        raise ValueError("canonical dimensions must stay within 16..512")
    output = require_e_temp(args.output, must_exist=False)
    if output.exists():
        raise ValueError(f"refusing to overwrite existing mouth atlas: {output}")

    references = {
        label: load_reference(
            label,
            getattr(args, f"{label}_image"),
            getattr(args, f"{label}_landmarks"),
        )
        for label in ("neutral", "open", "rounded", "spread", "labiodental")
    }
    if aperture(references["open"].record) < 0.12:
        raise ValueError("open reference does not contain enough articulation")
    if aperture(references["rounded"].record) > 0.09:
        raise ValueError("rounded reference is too open for a compact oo state")

    patches = {
        label: normalized_oral_patch(reference, args.width, args.height)
        for label, reference in references.items()
    }
    state_pixels = b"".join(patches[label].tobytes(order="C") for label, _ in STATE_LAYOUT)
    stride = args.width * 4
    state_bytes = stride * args.height
    if len(state_pixels) != len(STATE_LAYOUT) * state_bytes:
        raise ValueError("canonical texture byte count is inconsistent")

    output.mkdir(parents=True)
    texture_path = output / "atlas-bgra8-premultiplied.bin"
    texture_path.write_bytes(state_pixels)

    preview_scale = 2
    label_height = 28
    preview = np.full(
        (2 * (args.height * preview_scale + label_height), 4 * args.width * preview_scale, 3),
        (24, 27, 32),
        dtype=np.uint8,
    )
    for index, (label, _) in enumerate(STATE_LAYOUT):
        patch = patches[label]
        alpha = patch[..., 3:4].astype(np.float32) / 255.0
        background = np.full(patch[..., :3].shape, (36, 40, 47), dtype=np.float32)
        composited = np.clip(
            patch[..., :3].astype(np.float32) + background * (1.0 - alpha), 0, 255
        ).astype(np.uint8)
        composited = cv2.resize(
            composited,
            (args.width * preview_scale, args.height * preview_scale),
            interpolation=cv2.INTER_NEAREST,
        )
        column = index % 4
        row = index // 4
        x0 = column * args.width * preview_scale
        y0 = row * (args.height * preview_scale + label_height)
        preview[y0:y0 + composited.shape[0], x0:x0 + composited.shape[1]] = composited
        cv2.putText(
            preview,
            f"{index}: {label}",
            (x0 + 8, y0 + composited.shape[0] + 20),
            cv2.FONT_HERSHEY_SIMPLEX,
            0.52,
            (226, 231, 238),
            1,
            cv2.LINE_AA,
        )
    preview_path = output / "atlas-preview.png"
    if not cv2.imwrite(str(preview_path), preview):
        raise OSError(f"cannot write {preview_path}")
    identity_seed = "\n".join(
        f"{label}:{sha256_file(reference.image_path)}:{sha256_file(reference.landmark_path)}"
        for label, reference in sorted(references.items())
    ).encode("utf-8")
    identity_revision = int(sha256_bytes(identity_seed)[:16], 16) or 1
    manifest = {
        "schemaVersion": 1,
        "identityRevision": identity_revision,
        "texture": {
            "file": texture_path.name,
            "sha256": sha256_file(texture_path),
            "width": args.width,
            "height": args.height,
            "strideBytes": stride,
            "stateCount": len(STATE_LAYOUT),
            "stateBytes": state_bytes,
        },
        "states": [
            {
                "index": index,
                "coefficients": list(coefficients),
                "enrolledPose": [0.0, 0.0, 0.0],
            }
            for index, (_, coefficients) in enumerate(STATE_LAYOUT)
        ],
    }
    manifest_path = output / "atlas.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    evidence = {
        "schema": "interactive-npcs-generated-mouth-atlas-enrollment/v1",
        "runtimeUse": "canonical-mouth-pixels-only; source references are not required",
        "heavyModelRequiredDuringGameplay": False,
        "oralCanonicalBounds": list(ORAL_CANONICAL_BOUNDS),
        "stateReferences": [label for label, _ in STATE_LAYOUT],
        "references": {
            label: {
                "imageSha256": sha256_file(reference.image_path),
                "landmarksSha256": sha256_file(reference.landmark_path),
                "aperture": round(aperture(reference.record), 6),
            }
            for label, reference in sorted(references.items())
        },
        "artifacts": {
            "atlasManifestSha256": sha256_file(manifest_path),
            "textureSha256": sha256_file(texture_path),
            "previewSha256": sha256_file(preview_path),
        },
    }
    evidence_path = output / "enrollment-evidence.json"
    evidence_path.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({
        "status": "prepared",
        "output": str(output),
        "states": len(STATE_LAYOUT),
        "textureBytes": texture_path.stat().st_size,
        "textureSha256": manifest["texture"]["sha256"],
        "manifestSha256": sha256_file(manifest_path),
        "enrollmentEvidenceSha256": sha256_file(evidence_path),
        "preview": str(preview_path),
        "heavyModelRequiredDuringGameplay": False,
    }))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (cv2.error, json.JSONDecodeError, OSError, ValueError) as error:
        print(f"generated mouth atlas preparation error: {error}", file=sys.stderr)
        sys.exit(2)
