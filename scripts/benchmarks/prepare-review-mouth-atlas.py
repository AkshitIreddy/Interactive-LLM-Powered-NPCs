#!/usr/bin/env python3
"""Convert the measured Mara teacher atlas into the native review wire layout.

The source artifact stays outside the repository on E:\\temp.  This tool checks
its byte count, SHA-256, BGRA premultiplication, and state layout before creating
the tiny review bundle consumed by the Windows-native supervisor.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys


# State labels were assigned by inspecting the source-preserving teacher atlas.
# They use the same eight-dimensional order as native MouthCoefficients:
# jaw, close, funnel, pucker, smile-left, smile-right, upper-raise, lower-depress.
STATE_COEFFICIENTS = [
    [0.00, 1.00, 0.00, 0.00, 0.00, 0.00, 0.00, 0.00],  # neutral / silence
    [0.00, 0.48, 0.00, 0.00, 0.00, 0.00, 0.00, 0.18],  # labiodental
    [0.38, 0.00, 0.82, 0.76, 0.00, 0.00, 0.00, 0.00],  # rounded
    [0.24, 0.00, 0.00, 0.00, 0.00, 0.00, 0.28, 0.00],  # dental
    [0.92, 0.00, 0.00, 0.00, 0.00, 0.00, 0.32, 0.68],  # open vowel
    [0.32, 0.00, 0.00, 0.00, 0.12, 0.12, 0.00, 0.00],  # alveolar
    [0.52, 0.00, 0.00, 0.00, 0.76, 0.76, 0.00, 0.00],  # spread vowel
    [0.36, 0.00, 0.30, 0.00, 0.00, 0.00, 0.00, 0.00],  # postalveolar
]


def require_e_temp(path: Path) -> Path:
    resolved = path.resolve()
    if resolved.drive.lower() != "e:" or "temp" not in [part.lower() for part in resolved.parts]:
        raise ValueError("review atlas inputs and outputs must stay under E:\\temp")
    return resolved


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    source = require_e_temp(args.source)
    output = require_e_temp(args.output)
    if output.exists():
        raise ValueError(f"refusing to overwrite existing review atlas: {output}")

    source_manifest_path = source / "atlas-manifest.json"
    source_manifest = json.loads(source_manifest_path.read_text(encoding="utf-8"))
    texture = source_manifest["texture"]
    if source_manifest.get("artifactType") != "character-mouth-atlas":
        raise ValueError("source is not a character mouth atlas")
    state_count = int(texture["stateCount"])
    width = int(texture["width"])
    height = int(texture["height"])
    stride = int(texture["strideBytes"])
    state_bytes = int(texture["stateBytes"])
    if state_count != len(STATE_COEFFICIENTS) or state_bytes != stride * height:
        raise ValueError("source atlas state layout does not match the reviewed mapping")

    source_texture = source / str(texture["file"])
    pixels = source_texture.read_bytes()
    if len(pixels) != state_count * state_bytes or sha256(pixels) != texture["sha256"]:
        raise ValueError("source atlas byte count or SHA-256 changed")
    for offset in range(0, len(pixels), 4):
        blue, green, red, alpha = pixels[offset : offset + 4]
        if blue > alpha or green > alpha or red > alpha:
            raise ValueError(f"source atlas is not premultiplied at byte {offset}")

    output.mkdir(parents=True)
    destination_texture = output / "atlas-bgra8-premultiplied.bin"
    shutil.copyfile(source_texture, destination_texture)
    identity_hash = str(source_manifest["characterBinding"]["identityRevision"])
    identity_revision = int(identity_hash[:16], 16) or 1
    review_manifest = {
        "schemaVersion": 1,
        "identityRevision": identity_revision,
        "texture": {
            "file": destination_texture.name,
            "sha256": sha256(destination_texture.read_bytes()),
            "width": width,
            "height": height,
            "strideBytes": stride,
            "stateCount": state_count,
            "stateBytes": state_bytes,
        },
        "states": [
            {
                "index": index,
                "coefficients": coefficients,
                "enrolledPose": [0.0, 0.0, 0.0],
            }
            for index, coefficients in enumerate(STATE_COEFFICIENTS)
        ],
    }
    manifest_path = output / "atlas.json"
    manifest_path.write_text(json.dumps(review_manifest, indent=2) + "\n", encoding="utf-8")
    print(
        json.dumps(
            {
                "status": "prepared",
                "output": str(output),
                "states": state_count,
                "textureBytes": len(pixels),
                "textureSha256": review_manifest["texture"]["sha256"],
                "manifestSha256": sha256(manifest_path.read_bytes()),
            }
        )
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"review mouth atlas preparation error: {error}", file=sys.stderr)
        sys.exit(2)
