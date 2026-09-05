#!/usr/bin/env python3
"""Reject detached, face-distorting, or poorly articulated mouth residuals.

This fixture-level audit complements the native correctness tests.  It compares
rendered P6 PPM frames with their moving source frames, isolates the changed
mouth residual, and rejects the geometric signatures seen in failed renders:

* most active columns beginning or ending on one identical horizontal row;
* no materially open frame becoming tall enough relative to mouth width;
* a broad patch replacing skin outside the tracked outer-lip contour;
* weak correlation between requested and rendered aperture;
* mouth-width or roll drift relative to the moving source; and
* discontinuous identity-atlas state shapes under stable input.

The audit intentionally does not claim perceptual realism.  It is a narrow
regression gate for the pasted-on horizontal cavity that passed the older
whole-mouth motion metric.
"""

from __future__ import annotations

import argparse
from collections import Counter
import json
import math
from pathlib import Path
import statistics
import sys


def _token(stream) -> bytes:
    token = bytearray()
    while True:
        byte = stream.read(1)
        if not byte:
            raise ValueError("unexpected end of PPM header")
        if byte == b"#":
            stream.readline()
            continue
        if not byte.isspace():
            token.extend(byte)
            break
    while True:
        byte = stream.read(1)
        if not byte or byte.isspace():
            return bytes(token)
        token.extend(byte)


def read_ppm(path: Path) -> tuple[int, int, bytes]:
    with path.open("rb") as stream:
        if _token(stream) != b"P6":
            raise ValueError(f"{path} is not a binary P6 PPM")
        width = int(_token(stream))
        height = int(_token(stream))
        maximum = int(_token(stream))
        if maximum != 255:
            raise ValueError(f"{path} uses unsupported PPM maximum {maximum}")
        pixels = stream.read()
    expected = width * height * 3
    if len(pixels) != expected:
        raise ValueError(f"{path} has {len(pixels)} pixel bytes; expected {expected}")
    return width, height, pixels


def parse_roi(value: str) -> tuple[int, int, int, int]:
    parts = [int(part.strip()) for part in value.split(",")]
    if len(parts) != 4:
        raise argparse.ArgumentTypeError("ROI must be x0,y0,x1,y1")
    x0, y0, x1, y1 = parts
    if x0 < 0 or y0 < 0 or x1 <= x0 or y1 <= y0:
        raise argparse.ArgumentTypeError("ROI bounds must be positive and ordered")
    return x0, y0, x1, y1


def load_landmarks(path: Path | None) -> dict[str, dict]:
    if path is None:
        return {}
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("schema") != "interactive-npcs-mediapipe-mouth-landmarks/v1":
        raise ValueError(f"unsupported landmark schema in {path}")
    return {
        str(frame["file"]): frame
        for frame in document.get("frames", [])
        if frame.get("accepted")
    }


def percentile(values: list[float], quantile: float) -> float:
    """Deterministic Type-7 percentile, matching NumPy's linear default."""

    if not values:
        raise ValueError("cannot calculate a percentile of no values")
    ordered = sorted(float(value) for value in values)
    position = (len(ordered) - 1) * quantile
    lower = int(math.floor(position))
    upper = int(math.ceil(position))
    if lower == upper:
        return ordered[lower]
    amount = position - lower
    return ordered[lower] * (1.0 - amount) + ordered[upper] * amount


def average_ranks(values: list[float]) -> list[float]:
    order = sorted(range(len(values)), key=lambda index: (values[index], index))
    ranks = [0.0] * len(values)
    cursor = 0
    while cursor < len(order):
        end = cursor + 1
        while end < len(order) and values[order[end]] == values[order[cursor]]:
            end += 1
        rank = (cursor + end - 1) * 0.5
        for position in range(cursor, end):
            ranks[order[position]] = rank
        cursor = end
    return ranks


def correlation(first: list[float], second: list[float]) -> float:
    if len(first) != len(second) or len(first) < 2:
        raise ValueError("correlation requires two equally sized non-trivial series")
    first_mean = statistics.fmean(first)
    second_mean = statistics.fmean(second)
    first_delta = [value - first_mean for value in first]
    second_delta = [value - second_mean for value in second]
    denominator = math.sqrt(
        sum(value * value for value in first_delta)
        * sum(value * value for value in second_delta)
    )
    if denominator <= 1.0e-12:
        return 0.0
    return sum(a * b for a, b in zip(first_delta, second_delta)) / denominator


def spearman(first: list[float], second: list[float]) -> float:
    return correlation(average_ranks(first), average_ranks(second))


def point_pixels(record: dict, point: list[float]) -> tuple[float, float]:
    return (
        float(point[0]) * float(record["width"]),
        float(point[1]) * float(record["height"]),
    )


def corner_width_pixels(record: dict) -> float:
    return float(record["cornerWidth"]) * float(record["width"])


def rendered_aperture(record: dict) -> float:
    upper = [point_pixels(record, point) for point in record["innerUpper"]]
    lower = [point_pixels(record, point) for point in record["innerLower"]]
    gap = statistics.fmean(
        lower[index][1] - upper[index][1] for index in range(2, 9)
    )
    return max(0.0, gap / max(1.0, corner_width_pixels(record)))


def roll_error_degrees(source: dict, output: dict) -> float:
    delta = float(output["rollRadians"]) - float(source["rollRadians"])
    return abs(math.atan2(math.sin(delta), math.cos(delta))) * 180.0 / math.pi


def canonical_outer_lip(record: dict) -> list[tuple[float, float]]:
    width = float(record["width"])
    height = float(record["height"])
    center_x = float(record["center"][0]) * width
    center_y = float(record["center"][1]) * height
    angle = -float(record["rollRadians"])
    cosine = math.cos(angle)
    sine = math.sin(angle)
    scale = max(1.0, corner_width_pixels(record))
    points = outer_lip_polygon(record, int(width), int(height))
    return [
        (
            ((x - center_x) * cosine - (y - center_y) * sine) / scale,
            ((x - center_x) * sine + (y - center_y) * cosine) / scale,
        )
        for x, y in points
    ]


def canonical_shape_jump(first: dict, second: dict) -> float:
    first_points = canonical_outer_lip(first)
    second_points = canonical_outer_lip(second)
    if len(first_points) != len(second_points):
        raise ValueError("atlas states have different outer-lip point counts")
    return math.sqrt(
        statistics.fmean(
            (second_x - first_x) ** 2 + (second_y - first_y) ** 2
            for (first_x, first_y), (second_x, second_y)
            in zip(first_points, second_points)
        )
    )


def outer_lip_polygon(record: dict, width: int, height: int) -> list[tuple[float, float]]:
    upper = record.get("outerUpper")
    lower = record.get("outerLower")
    if not isinstance(upper, list) or not isinstance(lower, list) or len(upper) < 3 or len(lower) < 3:
        raise ValueError(f"landmark record {record.get('file', '<unknown>')} has no outer lip")
    return [
        (float(point[0]) * width, float(point[1]) * height)
        for point in upper
    ] + [
        (float(point[0]) * width, float(point[1]) * height)
        for point in reversed(lower[1:-1])
    ]


def point_inside_polygon(x: float, y: float, polygon: list[tuple[float, float]]) -> bool:
    inside = False
    previous_x, previous_y = polygon[-1]
    for current_x, current_y in polygon:
        crosses = (current_y > y) != (previous_y > y)
        if crosses:
            intersection_x = (
                (previous_x - current_x) * (y - current_y)
                / (previous_y - current_y)
                + current_x
            )
            if x < intersection_x:
                inside = not inside
        previous_x, previous_y = current_x, current_y
    return inside


def squared_distance_to_segment(
    x: float,
    y: float,
    first: tuple[float, float],
    second: tuple[float, float],
) -> float:
    dx = second[0] - first[0]
    dy = second[1] - first[1]
    denominator = dx * dx + dy * dy
    if denominator <= 1.0e-12:
        return (x - first[0]) ** 2 + (y - first[1]) ** 2
    amount = max(
        0.0,
        min(1.0, ((x - first[0]) * dx + (y - first[1]) * dy) / denominator),
    )
    projected_x = first[0] + amount * dx
    projected_y = first[1] + amount * dy
    return (x - projected_x) ** 2 + (y - projected_y) ** 2


def inside_expanded_lip(
    x: float,
    y: float,
    polygon: list[tuple[float, float]],
    margin: float,
) -> bool:
    if point_inside_polygon(x, y, polygon):
        return True
    limit = margin * margin
    return any(
        squared_distance_to_segment(x, y, first, second) <= limit
        for first, second in zip(polygon, polygon[1:] + polygon[:1])
    )


def analyse_pair(
    source_path: Path,
    output_path: Path,
    roi: tuple[int, int, int, int],
    difference_threshold: int,
    landmark_record: dict | None,
    lip_margin_ratio: float,
) -> dict[str, float | int | str] | None:
    source_width, source_height, source = read_ppm(source_path)
    output_width, output_height, output = read_ppm(output_path)
    if (source_width, source_height) != (output_width, output_height):
        raise ValueError(f"frame size mismatch: {source_path} vs {output_path}")

    x0, y0, x1, y1 = roi
    if x1 > source_width or y1 > source_height:
        raise ValueError(f"ROI {roi} exceeds {source_width}x{source_height} frames")

    top_by_x: dict[int, int] = {}
    bottom_by_x: dict[int, int] = {}
    changed = 0
    minimum_x = x1
    maximum_x = x0
    minimum_y = y1
    maximum_y = y0
    outside_lip = 0
    polygon: list[tuple[float, float]] = []
    lip_margin = 0.0
    if landmark_record is not None:
        polygon = outer_lip_polygon(landmark_record, source_width, source_height)
        lip_margin = max(
            2.0,
            float(landmark_record["cornerWidth"]) * source_width * lip_margin_ratio,
        )

    for y in range(y0, y1):
        row = y * source_width * 3
        for x in range(x0, x1):
            offset = row + x * 3
            delta = max(
                abs(source[offset] - output[offset]),
                abs(source[offset + 1] - output[offset + 1]),
                abs(source[offset + 2] - output[offset + 2]),
            )
            if delta <= difference_threshold:
                continue
            changed += 1
            if polygon and not inside_expanded_lip(x + 0.5, y + 0.5, polygon, lip_margin):
                outside_lip += 1
            minimum_x = min(minimum_x, x)
            maximum_x = max(maximum_x, x)
            minimum_y = min(minimum_y, y)
            maximum_y = max(maximum_y, y)
            top_by_x[x] = min(top_by_x.get(x, y), y)
            bottom_by_x[x] = max(bottom_by_x.get(x, y), y)

    if changed == 0:
        return None

    active_columns = len(top_by_x)
    top_mode_count = Counter(top_by_x.values()).most_common(1)[0][1]
    bottom_mode_count = Counter(bottom_by_x.values()).most_common(1)[0][1]
    residual_width = maximum_x - minimum_x + 1
    residual_height = maximum_y - minimum_y + 1
    result: dict[str, float | int | str] = {
        "source": source_path.name,
        "output": output_path.name,
        "changedPixels": changed,
        "residualWidth": residual_width,
        "residualHeight": residual_height,
        "heightOverWidth": residual_height / residual_width,
        "topEdgeModeShare": top_mode_count / active_columns,
        "bottomEdgeModeShare": bottom_mode_count / active_columns,
    }
    if polygon:
        result["outsideExpandedLipPixels"] = outside_lip
        result["outsideExpandedLipShare"] = outside_lip / changed
        result["lipMarginPixels"] = lip_margin
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--source-index-offset", type=int, default=1)
    parser.add_argument("--roi", type=parse_roi, default=(420, 340, 560, 410))
    parser.add_argument("--difference-threshold", type=int, default=5)
    parser.add_argument("--material-pixels", type=int, default=250)
    parser.add_argument("--minimum-material-frames", type=int, default=8)
    parser.add_argument("--maximum-edge-mode-share", type=float, default=0.65)
    parser.add_argument("--minimum-open-height-ratio", type=float, default=0.24)
    parser.add_argument("--landmarks", type=Path)
    parser.add_argument("--output-landmarks", type=Path)
    parser.add_argument("--atlas-landmarks", type=Path)
    parser.add_argument("--proof-manifest", type=Path)
    parser.add_argument("--lip-margin-ratio", type=float, default=0.08)
    parser.add_argument("--maximum-outside-lip-share", type=float, default=0.12)
    parser.add_argument("--maximum-p95-outside-lip-share", type=float, default=0.12)
    parser.add_argument("--maximum-frame-outside-lip-share", type=float, default=0.15)
    parser.add_argument("--active-openness-threshold", type=float, default=0.075)
    parser.add_argument("--minimum-active-frames", type=int, default=12)
    parser.add_argument("--minimum-aperture-spearman", type=float, default=0.85)
    parser.add_argument("--minimum-output-landmark-coverage", type=float, default=0.95)
    parser.add_argument("--minimum-corner-width-ratio-p05", type=float, default=0.90)
    parser.add_argument("--maximum-corner-width-ratio-p95", type=float, default=1.10)
    parser.add_argument("--minimum-corner-width-ratio", type=float, default=0.85)
    parser.add_argument("--maximum-corner-width-ratio", type=float, default=1.15)
    parser.add_argument("--maximum-roll-error-p95-degrees", type=float, default=2.5)
    parser.add_argument("--maximum-roll-error-degrees", type=float, default=4.0)
    parser.add_argument("--stable-openness-delta", type=float, default=0.10)
    parser.add_argument("--maximum-state-shape-jump-p95", type=float, default=0.03)
    parser.add_argument("--maximum-state-shape-jump", type=float, default=0.05)
    parser.add_argument("--output-json", type=Path)
    args = parser.parse_args()

    output_frames = sorted(args.output_dir.glob("frame-*.ppm"))
    if not output_frames:
        raise ValueError(f"no frame-*.ppm files in {args.output_dir}")

    landmark_records = load_landmarks(args.landmarks)
    output_landmark_records = load_landmarks(args.output_landmarks)
    atlas_landmark_records = load_landmarks(args.atlas_landmarks)
    proof = (
        json.loads(args.proof_manifest.read_text(encoding="utf-8"))
        if args.proof_manifest is not None
        else None
    )
    if (proof is None) != (not output_landmark_records):
        raise ValueError("--proof-manifest and --output-landmarks must be supplied together")
    frames: list[dict[str, float | int | str]] = []
    for output_path in output_frames:
        output_index = int(output_path.stem.split("-")[-1])
        source_index = output_index + args.source_index_offset
        source_path = args.source_dir / f"frame-{source_index:05d}.ppm"
        if not source_path.is_file():
            raise FileNotFoundError(source_path)
        result = analyse_pair(
            source_path,
            output_path,
            args.roi,
            args.difference_threshold,
            landmark_records.get(source_path.name),
            args.lip_margin_ratio,
        )
        if result is not None and result["changedPixels"] >= args.material_pixels:
            frames.append(result)

    failures: list[str] = []
    if len(frames) < args.minimum_material_frames:
        failures.append(
            f"only {len(frames)} materially articulated frames; "
            f"expected at least {args.minimum_material_frames}"
        )

    if frames:
        median_top = statistics.median(float(frame["topEdgeModeShare"]) for frame in frames)
        median_bottom = statistics.median(
            float(frame["bottomEdgeModeShare"]) for frame in frames
        )
        maximum_open = max(float(frame["heightOverWidth"]) for frame in frames)
        if median_top > args.maximum_edge_mode_share:
            failures.append(
                f"median top-edge scanline share {median_top:.3f} exceeds "
                f"{args.maximum_edge_mode_share:.3f}"
            )
        if median_bottom > args.maximum_edge_mode_share:
            failures.append(
                f"median bottom-edge scanline share {median_bottom:.3f} exceeds "
                f"{args.maximum_edge_mode_share:.3f}"
            )
        if maximum_open < args.minimum_open_height_ratio:
            failures.append(
                f"maximum residual height/width {maximum_open:.3f} is below "
                f"{args.minimum_open_height_ratio:.3f}"
            )
        confinement_frames = [
            frame for frame in frames if "outsideExpandedLipShare" in frame
        ]
        median_outside_lip = (
            statistics.median(
                float(frame["outsideExpandedLipShare"])
                for frame in confinement_frames
            )
            if confinement_frames
            else None
        )
        if landmark_records and len(confinement_frames) != len(frames):
            failures.append(
                f"only {len(confinement_frames)} of {len(frames)} material frames "
                "have accepted lip landmarks"
            )
        outside_values = [
            float(frame["outsideExpandedLipShare"])
            for frame in confinement_frames
        ]
        p95_outside_lip = percentile(outside_values, 0.95) if outside_values else None
        maximum_outside_lip = max(outside_values) if outside_values else None
        if (
            median_outside_lip is not None
            and median_outside_lip > args.maximum_outside_lip_share
        ):
            failures.append(
                f"median changed-pixel share outside expanded lip "
                f"{median_outside_lip:.3f} exceeds "
                f"{args.maximum_outside_lip_share:.3f}"
            )
        if (
            p95_outside_lip is not None
            and p95_outside_lip > args.maximum_p95_outside_lip_share
        ):
            failures.append(
                f"p95 changed-pixel share outside expanded lip "
                f"{p95_outside_lip:.3f} exceeds "
                f"{args.maximum_p95_outside_lip_share:.3f}"
            )
        if (
            maximum_outside_lip is not None
            and maximum_outside_lip > args.maximum_frame_outside_lip_share
        ):
            failures.append(
                f"maximum changed-pixel share outside expanded lip "
                f"{maximum_outside_lip:.3f} exceeds "
                f"{args.maximum_frame_outside_lip_share:.3f}"
            )
    else:
        median_top = 0.0
        median_bottom = 0.0
        maximum_open = 0.0
        median_outside_lip = None
        p95_outside_lip = None
        maximum_outside_lip = None

    articulation = None
    corner_width_report = None
    roll_report = None
    temporal_report = None
    if proof is not None:
        requested = [float(value) for value in proof.get("requestedOpenness", [])]
        selected = [int(value) for value in proof.get("selectedStateIndices", [])]
        if len(requested) != len(output_frames) or len(selected) != len(output_frames):
            raise ValueError("proof timeline length does not match output frame count")

        active_indices = [
            index
            for index, value in enumerate(requested)
            if value >= args.active_openness_threshold
        ]
        accepted_active = [
            index
            for index in active_indices
            if output_frames[index].name in output_landmark_records
        ]
        coverage = (
            len(accepted_active) / len(active_indices) if active_indices else 0.0
        )
        requested_active = [requested[index] for index in accepted_active]
        rendered_active = [
            rendered_aperture(output_landmark_records[output_frames[index].name])
            for index in accepted_active
        ]
        aperture_spearman = (
            spearman(requested_active, rendered_active)
            if len(accepted_active) >= 2
            else 0.0
        )
        articulation_passed = True
        if len(active_indices) < args.minimum_active_frames:
            articulation_passed = False
            failures.append(
                f"only {len(active_indices)} active openness frames; "
                f"expected at least {args.minimum_active_frames}"
            )
        if coverage < args.minimum_output_landmark_coverage:
            articulation_passed = False
            failures.append(
                f"active output landmark coverage {coverage:.3f} is below "
                f"{args.minimum_output_landmark_coverage:.3f}"
            )
        if aperture_spearman < args.minimum_aperture_spearman:
            articulation_passed = False
            failures.append(
                f"active rendered-aperture Spearman {aperture_spearman:.3f} is below "
                f"{args.minimum_aperture_spearman:.3f}"
            )
        articulation = {
            "eligibleFrames": len(active_indices),
            "acceptedFrames": len(accepted_active),
            "outputLandmarkCoverage": round(coverage, 6),
            "activeOpennessThreshold": args.active_openness_threshold,
            "renderedApertureRequestedSpearman": round(aperture_spearman, 6),
            "minimumSpearman": args.minimum_aperture_spearman,
            "maximumRenderedAperture": (
                round(max(rendered_active), 6) if rendered_active else 0.0
            ),
            "passed": articulation_passed,
        }

        geometry_pairs = []
        for frame in frames:
            source_record = landmark_records.get(str(frame["source"]))
            output_record = output_landmark_records.get(str(frame["output"]))
            if source_record is not None and output_record is not None:
                geometry_pairs.append((source_record, output_record))
        if landmark_records:
            geometry_coverage = len(geometry_pairs) / len(frames) if frames else 0.0
            if geometry_coverage < args.minimum_output_landmark_coverage:
                failures.append(
                    f"material output/source landmark coverage {geometry_coverage:.3f} is below "
                    f"{args.minimum_output_landmark_coverage:.3f}"
                )
            width_ratios = [
                corner_width_pixels(output_record) / max(1.0, corner_width_pixels(source_record))
                for source_record, output_record in geometry_pairs
            ]
            if width_ratios:
                width_p05 = percentile(width_ratios, 0.05)
                width_p95 = percentile(width_ratios, 0.95)
                width_minimum = min(width_ratios)
                width_maximum = max(width_ratios)
                width_passed = (
                    width_p05 >= args.minimum_corner_width_ratio_p05
                    and width_p95 <= args.maximum_corner_width_ratio_p95
                    and width_minimum >= args.minimum_corner_width_ratio
                    and width_maximum <= args.maximum_corner_width_ratio
                )
                if not width_passed:
                    failures.append(
                        "rendered/source corner-width ratios exceed bounded percentiles or hard limits"
                    )
                corner_width_report = {
                    "frames": len(width_ratios),
                    "minimum": round(width_minimum, 6),
                    "p05": round(width_p05, 6),
                    "median": round(statistics.median(width_ratios), 6),
                    "p95": round(width_p95, 6),
                    "maximum": round(width_maximum, 6),
                    "passed": width_passed,
                }

                roll_errors = [
                    roll_error_degrees(source_record, output_record)
                    for source_record, output_record in geometry_pairs
                ]
                roll_p95 = percentile(roll_errors, 0.95)
                roll_maximum = max(roll_errors)
                roll_passed = (
                    roll_p95 <= args.maximum_roll_error_p95_degrees
                    and roll_maximum <= args.maximum_roll_error_degrees
                )
                if not roll_passed:
                    failures.append(
                        "rendered/source mouth-roll error exceeds bounded p95 or hard limit"
                    )
                roll_report = {
                    "frames": len(roll_errors),
                    "median": round(statistics.median(roll_errors), 6),
                    "p95": round(roll_p95, 6),
                    "maximum": round(roll_maximum, 6),
                    "passed": roll_passed,
                }

        if atlas_landmark_records:
            jumps = []
            for index in range(1, len(selected)):
                openness_delta = abs(requested[index] - requested[index - 1])
                visibility = min(
                    min(1.0, requested[index - 1] / 0.075),
                    min(1.0, requested[index] / 0.075),
                )
                if openness_delta > args.stable_openness_delta or visibility <= 0.0:
                    continue
                first_name = f"frame-{selected[index - 1]:05d}.ppm"
                second_name = f"frame-{selected[index]:05d}.ppm"
                first_record = atlas_landmark_records.get(first_name)
                second_record = atlas_landmark_records.get(second_name)
                if first_record is None or second_record is None:
                    raise ValueError(f"selected atlas landmarks missing: {first_name}, {second_name}")
                jump = canonical_shape_jump(first_record, second_record) * visibility
                jumps.append({
                    "fromOutputFrame": index - 1,
                    "toOutputFrame": index,
                    "fromAtlasState": selected[index - 1],
                    "toAtlasState": selected[index],
                    "requestedOpennessDelta": openness_delta,
                    "visibleCanonicalShapeJump": jump,
                })
            jump_values = [float(item["visibleCanonicalShapeJump"]) for item in jumps]
            jump_p95 = percentile(jump_values, 0.95) if jump_values else 0.0
            jump_maximum = max(jump_values) if jump_values else 0.0
            jump_passed = (
                bool(jump_values)
                and jump_p95 <= args.maximum_state_shape_jump_p95
                and jump_maximum <= args.maximum_state_shape_jump
            )
            if not jump_passed:
                failures.append(
                    "stable-input atlas shape jump exceeds bounded p95 or hard limit"
                )
            worst = max(jumps, key=lambda item: item["visibleCanonicalShapeJump"]) if jumps else None
            if worst is not None:
                worst = {
                    key: round(value, 6) if isinstance(value, float) else value
                    for key, value in worst.items()
                }
            temporal_report = {
                "eligibleTransitions": len(jumps),
                "stableOpennessDelta": args.stable_openness_delta,
                "p95VisibleCanonicalShapeJump": round(jump_p95, 6),
                "maximumVisibleCanonicalShapeJump": round(jump_maximum, 6),
                "worstTransition": worst,
                "passed": jump_passed,
            }

    report = {
        "schema": "interactive-npcs-mouth-render-quality/v2",
        "status": "failed" if failures else "passed",
        "purpose": (
            "reject scanline-flat, broad-patch, weakly articulated, "
            "geometrically drifting, or discontinuous mouth residuals"
        ),
        "materialFrames": len(frames),
        "medianTopEdgeModeShare": round(median_top, 6),
        "medianBottomEdgeModeShare": round(median_bottom, 6),
        "maximumResidualHeightOverWidth": round(maximum_open, 6),
        "medianOutsideExpandedLipShare": (
            round(median_outside_lip, 6)
            if median_outside_lip is not None
            else None
        ),
        "p95OutsideExpandedLipShare": (
            round(p95_outside_lip, 6) if p95_outside_lip is not None else None
        ),
        "maximumOutsideExpandedLipShare": (
            round(maximum_outside_lip, 6) if maximum_outside_lip is not None else None
        ),
        "articulation": articulation,
        "cornerWidthRatio": corner_width_report,
        "mouthRollErrorDegrees": roll_report,
        "temporalStatePopping": temporal_report,
        "thresholds": {
            "maximumEdgeModeShare": args.maximum_edge_mode_share,
            "minimumOpenHeightRatio": args.minimum_open_height_ratio,
            "lipMarginRatio": args.lip_margin_ratio,
            "maximumOutsideLipShare": args.maximum_outside_lip_share,
            "maximumP95OutsideLipShare": args.maximum_p95_outside_lip_share,
            "maximumFrameOutsideLipShare": args.maximum_frame_outside_lip_share,
            "activeOpennessThreshold": args.active_openness_threshold,
            "minimumActiveFrames": args.minimum_active_frames,
            "minimumOutputLandmarkCoverage": args.minimum_output_landmark_coverage,
            "minimumApertureSpearman": args.minimum_aperture_spearman,
            "minimumCornerWidthRatioP05": args.minimum_corner_width_ratio_p05,
            "maximumCornerWidthRatioP95": args.maximum_corner_width_ratio_p95,
            "minimumCornerWidthRatio": args.minimum_corner_width_ratio,
            "maximumCornerWidthRatio": args.maximum_corner_width_ratio,
            "maximumRollErrorP95Degrees": args.maximum_roll_error_p95_degrees,
            "maximumRollErrorDegrees": args.maximum_roll_error_degrees,
            "stableOpennessDelta": args.stable_openness_delta,
            "maximumStateShapeJumpP95": args.maximum_state_shape_jump_p95,
            "maximumStateShapeJump": args.maximum_state_shape_jump,
        },
        "failures": failures,
        "frames": frames,
    }
    encoded = json.dumps(report, indent=2)
    if args.output_json is not None:
        args.output_json.parent.mkdir(parents=True, exist_ok=True)
        args.output_json.write_text(encoded + "\n", encoding="utf-8")
    print(encoded)
    return 1 if failures else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"quality audit error: {error}", file=sys.stderr)
        sys.exit(2)
