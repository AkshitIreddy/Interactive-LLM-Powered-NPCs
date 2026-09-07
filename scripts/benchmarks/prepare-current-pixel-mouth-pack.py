#!/usr/bin/env python3
"""Export an identity-bound schema-four oral strip pack for local qualification.

The optional reference contributes only its eroded oral interior. An omitted
reference makes a transparent, source-only pack. Enrollment defaults to
unreviewed; importing an enabled product pack requires a separate review hash.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re

import numpy as np


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--character", required=True)
    parser.add_argument("--game-profile", required=True)
    parser.add_argument("--source-provenance", type=Path, required=True)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--annotation", type=Path)
    parser.add_argument("--refine-source-edges", action="store_true")
    parser.add_argument("--review-evidence", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not all(re.fullmatch(r"[a-z0-9](?:[a-z0-9.-]{0,94}[a-z0-9])?", value)
               and ".." not in value
               for value in (args.character, args.game_profile)):
        parser.error("character and game profile must be semantic identifiers")
    if args.output.exists():
        parser.error("output must be a fresh directory")
    if args.annotation and not args.reference:
        parser.error("annotation requires its exact reference image")

    provenance = [sha(args.source_provenance)]
    texture = np.zeros((64, 128, 4), dtype=np.uint8)
    context_mean = 1.0
    if args.reference:
        script = Path(__file__).with_name("render-current-pixel-mouth-proof.py")
        spec = importlib.util.spec_from_file_location("current_pixel_reference", script)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        oral = module.enroll_oral_reference(args.reference, args.annotation)
        alpha = np.rint(oral["alpha"] * 255).astype(np.uint8)
        texture[..., :3] = np.rint(oral["texture"] * (alpha[..., None] / 255)).astype(np.uint8)
        texture[..., 3] = alpha
        context_mean = float(oral["contextMean"])
        provenance.append(sha(args.reference))
        if args.annotation:
            provenance.append(sha(args.annotation))
    # Four addresses satisfy the existing identity atlas selector contract.
    # They deliberately share one oral observation: geometry is continuous,
    # while reference teeth are never cross-faded between photographs.
    coefficients = [[0, 1, 0, 0, 0, 0, 0, 0], [1, 0, 0, 0, 0, 0, 0, 0],
                    [.75, 0, 1, 1, 0, 0, 0, 0], [.575, 0, 0, 0, 1, 1, 0, 0]]
    pixels = texture.tobytes() * len(coefficients)
    pixel_sha = hashlib.sha256(pixels).hexdigest()
    revision_seed = json.dumps([args.character, args.game_profile, provenance,
                               pixel_sha, args.refine_source_edges]).encode()
    revision = int.from_bytes(hashlib.sha256(revision_seed).digest()[:8], "little") or 1
    binding = {"schemaVersion": 1, "gameProfileId": args.game_profile,
               "characterId": args.character, "referenceProvenanceSha256": provenance,
               "reviewStatus": "reviewed-private" if args.review_evidence else "unreviewed"}
    if args.review_evidence:
        binding["reviewEvidenceSha256"] = sha(args.review_evidence)
    manifest = {"schemaVersion": 4, "identityRevision": revision,
                "enrollmentBinding": binding,
                "texture": {"file": "atlas-bgra8-premultiplied.bin", "sha256": pixel_sha,
                            "representation": "normalized-oral-strip-v1", "width": 128,
                            "height": 64, "strideBytes": 512, "stateCount": 4,
                            "stateBytes": 32768},
                "states": [{"index": i, "coefficients": values, "enrolledPose": [0, 0, 0],
                            "referenceContextMean": context_mean,
                            "refineSourceEdges": args.refine_source_edges}
                           for i, values in enumerate(coefficients)]}
    args.output.mkdir(parents=True)
    (args.output / "atlas-bgra8-premultiplied.bin").write_bytes(pixels)
    (args.output / "atlas.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.output), "textureSha256": pixel_sha,
                      "sourceOnly": not bool(args.reference), "reviewStatus": binding["reviewStatus"]}))


if __name__ == "__main__":
    main()
