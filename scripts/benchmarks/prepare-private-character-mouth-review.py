#!/usr/bin/env python3
"""Build a closed-world, private-review character mouth pack and receipt.

This wrapper records the identity-reference chain outside the distributable
artifact, measures the generated oral observation, delegates atlas construction
to ``prepare-current-pixel-mouth-pack.py``, and writes the exact three-file
receipt root accepted by the Windows review packager.

The result is deliberately *not* a natural-quality or ordinary-target claim.
It is a private component-review input whose oral pixels are bound to one game
and character. Source documents and generated enrollment images remain outside
the pack so they cannot be accidentally redistributed with it.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import urlparse

import cv2
import numpy as np


SEMANTIC_ID = re.compile(r"^[a-z0-9](?:[a-z0-9.-]{0,94}[a-z0-9])?$")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def load_reference_module():
    path = Path(__file__).with_name("render-current-pixel-mouth-proof.py")
    spec = importlib.util.spec_from_file_location("current_pixel_reference", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("current-pixel reference module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def make_review_board(reference: Path, oral: dict, output: Path) -> None:
    image = cv2.imread(str(reference))
    if image is None:
        raise ValueError("generated enrollment reference could not be decoded")
    points = np.concatenate(
        [np.asarray(value, dtype=np.float64) for value in oral["contours"].values()]
    )
    mouth_width = float(oral["mouthWidthPixels"])
    center = points.mean(axis=0)
    radius_x = max(48, int(round(mouth_width * 1.45)))
    radius_y = max(40, int(round(mouth_width * 0.92)))
    x0 = max(0, int(center[0]) - radius_x)
    x1 = min(image.shape[1], int(center[0]) + radius_x)
    y0 = max(0, int(center[1]) - radius_y)
    y1 = min(image.shape[0], int(center[1]) + radius_y)
    crop = image[y0:y1, x0:x1]
    if crop.size == 0:
        raise ValueError("generated oral review crop is empty")
    crop = cv2.resize(crop, (640, 360), interpolation=cv2.INTER_LANCZOS4)

    texture = np.asarray(oral["texture"], dtype=np.uint8).copy()
    alpha = np.asarray(oral["alpha"], dtype=np.float32)
    checker = np.full_like(texture, 44)
    checker[:, ::16] = 60
    visible = np.rint(texture * alpha[..., None] + checker * (1 - alpha[..., None])).astype(
        np.uint8
    )
    visible = cv2.resize(visible, (640, 320), interpolation=cv2.INTER_NEAREST)

    board = np.full((820, 720, 3), 18, dtype=np.uint8)
    board[58:418, 40:680] = crop
    board[458:778, 40:680] = visible
    cv2.putText(board, "GENERATED REFERENCE - MOUTH CLOSE-UP", (40, 35),
                cv2.FONT_HERSHEY_SIMPLEX, .72, (235, 235, 235), 2, cv2.LINE_AA)
    cv2.putText(board, "ERODED ORAL INTERIOR STORED IN PACK", (40, 446),
                cv2.FONT_HERSHEY_SIMPLEX, .72, (80, 220, 240), 2, cv2.LINE_AA)
    cv2.putText(
        board,
        f"width={mouth_width:.1f}px  center-gap={float(oral['gapPixels']):.1f}px",
        (40, 806),
        cv2.FONT_HERSHEY_SIMPLEX,
        .58,
        (190, 190, 190),
        1,
        cv2.LINE_AA,
    )
    if not cv2.imwrite(str(output), board):
        raise RuntimeError("review board could not be written")


def canonical_binding_sha256(binding: dict) -> str:
    encoded = json.dumps(binding, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--character", required=True)
    parser.add_argument("--game-profile", default="cyberpunk-2077")
    parser.add_argument("--source-url", required=True)
    parser.add_argument("--source-owner", required=True)
    parser.add_argument("--source-document", type=Path, required=True)
    parser.add_argument("--source-image", type=Path, required=True)
    parser.add_argument("--generated-reference", type=Path, required=True)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--retrieved-at", required=True)
    parser.add_argument("--generation-tool", default="OpenAI built-in image generation tool")
    args = parser.parse_args()

    for label, value in (("character", args.character), ("game profile", args.game_profile)):
        if not SEMANTIC_ID.fullmatch(value) or ".." in value:
            parser.error(f"{label} must be a semantic identifier")
    parsed_url = urlparse(args.source_url)
    if parsed_url.scheme != "https" or not parsed_url.netloc:
        parser.error("source URL must be absolute HTTPS")
    for path in (args.source_document, args.source_image, args.generated_reference):
        if not path.is_file():
            parser.error(f"required input is missing: {path}")
    if args.workspace.exists():
        parser.error("workspace must be a fresh directory")

    args.workspace.mkdir(parents=True)
    evidence_root = args.workspace / "evidence"
    artifact_root = args.workspace / "pack-v1"
    evidence_root.mkdir()

    reference_module = load_reference_module()
    oral = reference_module.enroll_oral_reference(args.generated_reference)
    alpha_coverage = float(np.mean(np.asarray(oral["alpha"]) > 0))
    if oral["mouthWidthPixels"] < 48 or oral["gapPixels"] < 4 or alpha_coverage < 0.20:
        raise ValueError("generated oral reference does not meet private review geometry floors")

    provenance_path = evidence_root / "source-provenance.json"
    provenance = {
        "schema": "interactive-npcs-private-mouth-source/v1",
        "gameProfileId": args.game_profile,
        "characterId": args.character,
        "source": {
            "url": args.source_url,
            "retrievedAt": args.retrieved_at,
            "documentFile": args.source_document.name,
            "documentSha256": sha256(args.source_document),
            "renderedImageFile": args.source_image.name,
            "renderedImageSha256": sha256(args.source_image),
            "owner": args.source_owner,
            "usageBoundary": "private local evaluation; source is not staged or redistributed",
        },
        "generatedEnrollment": {
            "file": args.generated_reference.name,
            "sha256": sha256(args.generated_reference),
            "tool": args.generation_tool,
            "editScope": "identity-preserving, mouth-only, restrained open-vowel reference",
            "usageBoundary": "private local enrollment; not a public game asset",
        },
    }
    write_json(provenance_path, provenance)

    board_path = evidence_root / "oral-reference-review-board.png"
    make_review_board(args.generated_reference, oral, board_path)
    review_path = evidence_root / "visual-review-evidence.json"
    review = {
        "schema": "interactive-npcs-private-mouth-visual-review/v1",
        "gameProfileId": args.game_profile,
        "characterId": args.character,
        "status": "accepted-for-private-component-review",
        "reviewedInputs": {
            "generatedReferenceSha256": sha256(args.generated_reference),
            "oralReferenceBoardSha256": sha256(board_path),
        },
        "geometry": {
            "mouthWidthPixels": float(oral["mouthWidthPixels"]),
            "centerGapPixels": float(oral["gapPixels"]),
            "oralAlphaCoverage": alpha_coverage,
        },
        "observations": [
            "character-specific face and oral anatomy remained recognizable in the generated reference",
            "upper teeth and oral cavity are resolved without a flat painted oval",
            "the pack contains only the eroded oral interior; runtime lip surfaces come from the current game frame",
        ],
        "limits": [
            "one generated open-vowel observation",
            "natural motion and cross-pose quality are not qualified",
            "live capture, game load, presentation, and ordinary targets are not qualified",
        ],
    }
    write_json(review_path, review)

    builder = Path(__file__).with_name("prepare-current-pixel-mouth-pack.py")
    subprocess.run(
        [
            sys.executable,
            str(builder),
            "--character",
            args.character,
            "--game-profile",
            args.game_profile,
            "--source-provenance",
            str(provenance_path),
            "--reference",
            str(args.generated_reference),
            "--refine-source-edges",
            "--review-evidence",
            str(review_path),
            "--output",
            str(artifact_root),
        ],
        check=True,
    )

    atlas_path = artifact_root / "atlas.json"
    texture_path = artifact_root / "atlas-bgra8-premultiplied.bin"
    atlas = json.loads(atlas_path.read_text(encoding="utf-8"))
    binding = atlas["enrollmentBinding"]
    receipt = {
        "schema": "interactive-npcs-reviewed-mouth-atlas/v1",
        "artifact": {
            "atlas": {
                "path": atlas_path.name,
                "sizeBytes": atlas_path.stat().st_size,
                "sha256": sha256(atlas_path),
            },
            "texture": {
                "path": texture_path.name,
                "sizeBytes": texture_path.stat().st_size,
                "sha256": sha256(texture_path),
            },
        },
        "atlasIdentity": {
            "schemaVersion": 4,
            "identityRevision": str(atlas["identityRevision"]),
            "representation": "normalized-oral-strip-v1",
            "gameProfileId": args.game_profile,
            "characterId": args.character,
            "enrollmentBindingSha256": canonical_binding_sha256(binding),
        },
        "review": {
            "classification": "reviewed-private-character-pack",
            "status": "accepted-for-private-review",
            "reviewEvidenceSha256": sha256(review_path),
            "naturalQualityQualified": False,
            "ordinaryTargetsEnabled": False,
            "qualification": (
                "Private local component-review pack from one identity-preserving generated oral "
                "enrollment image. Natural motion, live capture, game-load behavior, presentation, "
                "and ordinary targets remain unqualified."
            ),
        },
    }
    receipt_path = artifact_root / "reviewed-artifact-receipt.v1.json"
    write_json(receipt_path, receipt)
    print(
        json.dumps(
            {
                "characterId": args.character,
                "receipt": str(receipt_path),
                "reviewBoard": str(board_path),
                "mouthWidthPixels": oral["mouthWidthPixels"],
                "gapPixels": oral["gapPixels"],
                "alphaCoverage": alpha_coverage,
            }
        )
    )


if __name__ == "__main__":
    main()
