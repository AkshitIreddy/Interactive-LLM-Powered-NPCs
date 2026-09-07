#!/usr/bin/env python3
"""Offline, current-frame mouth deformation comparator; not a product runtime.

The renderer moves the target's own lip pixels, never an atlas's outer lips or
skin. Explicit face crops are review inputs, not automatic actor selection.
Audio categories come from a sample-clock cue file. Missing geometry, folded
surfaces and silence preserve the input. A private optional reference supplies
only unseen oral interior. Manual exclusions are explicitly not actor admission.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import platform
import time

import cv2
import mediapipe as mp
import numpy as np
import scipy
from scipy.spatial import Delaunay
from scipy.interpolate import PchipInterpolator

CONTOURS = {
    "outerUpper": [61, 185, 40, 39, 37, 0, 267, 269, 270, 409, 291],
    "outerLower": [61, 146, 91, 181, 84, 17, 314, 405, 321, 375, 291],
    "innerUpper": [78, 191, 80, 81, 82, 13, 312, 311, 310, 415, 308],
    "innerLower": [78, 95, 88, 178, 87, 14, 317, 402, 318, 324, 308],
}


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def crop_landmarks(image: np.ndarray, box: list[int], mesh) -> dict | None:
    x0, y0, x1, y1 = box
    h, w = image.shape[:2]
    if not (0 <= x0 < x1 <= w and 0 <= y0 < y1 <= h):
        raise ValueError("face crop outside source frame")
    # A review crop makes a small game face resolvable to the detector. No
    # super-resolution model or generated pixel enters the actual warp.
    crop = cv2.resize(image[y0:y1, x0:x1], (512, 512), interpolation=cv2.INTER_CUBIC)
    result = mesh.process(cv2.cvtColor(crop, cv2.COLOR_BGR2RGB))
    if not result.multi_face_landmarks:
        return None
    landmarks = result.multi_face_landmarks[0].landmark
    contours = {
        name: np.array([[x0 + landmarks[i].x * (x1-x0),
                         y0 + landmarks[i].y * (y1-y0)] for i in ids], np.float64)
        for name, ids in CONTOURS.items()
    }
    points = np.concatenate(list(contours.values()))
    width = np.linalg.norm(contours["outerUpper"][-1]-contours["outerUpper"][0])
    if not np.isfinite(points).all() or not 8 < width < (x1-x0)*0.8:
        return None
    return contours


def mouth_coordinates(contours: dict) -> tuple[np.ndarray, np.ndarray, float]:
    left, right = contours["outerUpper"][[0, -1]]
    width = float(np.linalg.norm(right-left))
    horizontal = (right-left)/width
    axes = np.stack([horizontal, [-horizontal[1], horizontal[0]]])
    return (left+right)*0.5, axes, width


def smooth_local_shape(contours: dict, previous: dict | None, fps: float) -> tuple[dict, dict]:
    """Damp contour noise without delaying current translation, roll or scale."""
    center, axes, width = mouth_coordinates(contours)
    current = {key: (value-center) @ axes.T/width for key, value in contours.items()}
    blend = 1-math.exp(-1/(fps*.025))
    filtered = current if previous is None else {
        key: value + np.clip((previous[key]-value)*(1-blend), -.015, .015)
        for key, value in current.items()}
    return {key: value*width @ axes+center for key, value in filtered.items()}, filtered


def bridge_geometry(previous_gray: np.ndarray, current_gray: np.ndarray,
                    previous_contours: dict) -> dict | None:
    """One-frame geometry bridge using untouched-frame forward/backward flow.

    This is a local tracking consistency check, not semantic occlusion proof.
    It never supplies a rendered pixel or permits a repeated prediction chain.
    """
    if previous_gray.shape != current_gray.shape:
        return None
    center, _, width = mouth_coordinates(previous_contours)
    cx, cy = center.astype(int)
    r = max(3, int(width*.65))
    patch = previous_gray[max(0,cy-r):cy+r, max(0,cx-r):cx+r]
    if patch.size == 0 or float(patch.std()) < 4:
        return None
    names = list(previous_contours)
    points = np.concatenate([previous_contours[key] for key in names]).astype(np.float32).reshape(-1,1,2)
    options = dict(winSize=(21,21), maxLevel=3,
                   criteria=(cv2.TERM_CRITERIA_EPS | cv2.TERM_CRITERIA_COUNT,30,.01))
    forward, good, _ = cv2.calcOpticalFlowPyrLK(previous_gray, current_gray, points, None, **options)
    direct_ok = forward is not None and good is not None and good.all()
    if direct_ok:
        backward, returned, _ = cv2.calcOpticalFlowPyrLK(current_gray, previous_gray, forward, None, **options)
        direct_ok = backward is not None and returned is not None and returned.all()
    if direct_ok:
        error = np.linalg.norm(backward-points, axis=2).ravel()
        movement = np.linalg.norm(forward-points, axis=2).ravel()
        direct_ok = (np.isfinite(forward).all() and np.median(error) <= .35
                     and np.max(error) <= 1.2 and movement.max() <= width*1.25)
    if not direct_ok:
        # Lip paint may lack texture even while current cheek/nose features
        # establish rigid face motion. Fit a short-lived similarity transform
        # with outlier rejection; never chain this across missed observations.
        mask = np.zeros_like(previous_gray)
        x0, x1 = max(0,int(cx-1.6*width)), min(mask.shape[1],int(cx+1.6*width))
        y0, y1 = max(0,int(cy-2.1*width)), min(mask.shape[0],int(cy+.8*width))
        mask[y0:y1,x0:x1] = 255
        features = cv2.goodFeaturesToTrack(previous_gray, 100, .015, 3, mask=mask)
        if features is None or len(features) < 12:
            return None
        target, status, _ = cv2.calcOpticalFlowPyrLK(previous_gray,current_gray,features,None,**options)
        if target is None:
            return None
        back, back_status, _ = cv2.calcOpticalFlowPyrLK(current_gray,previous_gray,target,None,**options)
        if back is None:
            return None
        keep = ((status.ravel()!=0) & (back_status.ravel()!=0)
                & (np.linalg.norm(back-features,axis=2).ravel()<.7))
        if int(keep.sum()) < 12:
            return None
        affine, inliers = cv2.estimateAffinePartial2D(features[keep],target[keep],
            method=cv2.RANSAC,ransacReprojThreshold=.8,maxIters=1000,confidence=.99)
        if affine is None or inliers is None or float(inliers.mean()) < .5 or int(inliers.sum()) < 24:
            return None
        support = features[keep][inliers.ravel()!=0,0,:]
        span = np.ptp(support,axis=0)
        if span[0] < width*.8 or span[1] < width*.5:
            return None
        scale = float(np.hypot(affine[0,0],affine[1,0]))
        angle = abs(math.atan2(affine[1,0],affine[0,0]))
        if not .88 <= scale <= 1.12 or angle > math.radians(10):
            return None
        forward = cv2.transform(points,affine)
        if not np.isfinite(forward).all() or np.max(np.linalg.norm(forward-points,axis=2)) > width*1.25:
            return None
    result, offset = {}, 0
    for key in names:
        count = len(previous_contours[key])
        result[key] = forward[offset:offset+count,0,:].astype(np.float64)
        offset += count
    return result


def control_points(contours: dict, target_aperture: float, target_width: float,
                   strength: float) -> tuple[np.ndarray, np.ndarray, dict]:
    center, axes, width = mouth_coordinates(contours)
    local = {key: (value-center) @ axes.T for key, value in contours.items()}
    source_gap = float(local["innerLower"][5, 1]-local["innerUpper"][5, 1])
    if source_gap < -0.5:
        raise ValueError("crossed source inner lips")
    # Preserve anatomical thickness: move upper and lower lips as separate
    # surfaces, rather than stretching a whole rectangular mouth texture.
    wanted = max(0.55, width*target_aperture)
    # A warp-only baseline has no unseen oral anatomy. Restrict the opening to
    # the available source gap; the cap is recorded rather than hidden.
    cap = max(1.8, source_gap*2.25)
    gap = source_gap + strength*(min(wanted, cap)-source_gap)
    delta = gap-source_gap
    deformed = {}
    for key, points in local.items():
        target = points.copy()
        bell = np.maximum(0, 1-(points[:, 0]/(width*.52))**2)**.8
        target[:, 0] *= 1+strength*(target_width-1)
        share = -.32 if "Upper" in key else .68
        target[:, 1] += share*delta*bell
        deformed[key] = target
    def unique_contours(value):
        return np.concatenate([value["outerUpper"], value["outerLower"][1:-1],
                               value["innerUpper"], value["innerLower"][1:-1]])
    original = unique_contours(local)
    target = unique_contours(deformed)
    # An unchanged outer support ring pins cheek, nose and jaw context. Two
    # rings allow deformation to decay smoothly before the exact boundary.
    anchors = []
    for sx, sy in ((.76, .48), (1.04, .72)):
        for angle in np.linspace(0, 2*math.pi, 16, endpoint=False):
            anchors.append([width*sx*math.cos(angle), width*sy*math.sin(angle)])
    original = np.concatenate([original, anchors])
    target = np.concatenate([target, anchors])
    return original @ axes+center, target @ axes+center, {
        "mouthWidthPixels": width, "sourceGapPixels": source_gap,
        "targetGapPixels": gap, "requestedGapPixels": wanted,
        "openingLimitedBySource": bool(wanted > cap),
    }


def warp_pixels(image: np.ndarray, original: np.ndarray, target: np.ndarray,
                triangles: np.ndarray) -> tuple[np.ndarray, list[int]]:
    height, width = image.shape[:2]
    lo = np.floor(np.minimum(original.min(0), target.min(0))-2).astype(int)
    hi = np.ceil(np.maximum(original.max(0), target.max(0))+2).astype(int)
    x0, y0 = np.maximum(lo, 0)
    x1, y1 = np.minimum(hi, [width, height])
    yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
    map_x, map_y = xx.copy(), yy.copy()
    moved = np.zeros(xx.shape, bool)
    for indices in triangles:
        src, dst = original[indices], target[indices]
        a = np.column_stack([dst[1]-dst[0], dst[2]-dst[0]])
        b = np.column_stack([src[1]-src[0], src[2]-src[0]])
        determinant, source_det = np.linalg.det(a), np.linalg.det(b)
        if abs(source_det) < .02:
            continue
        if source_det*determinant <= 0 or abs(determinant/source_det) < .025:
            raise ValueError("deformation folds or collapses a triangle")
        minxy = np.maximum(np.floor(dst.min(0)).astype(int), [x0, y0])
        maxxy = np.minimum(np.ceil(dst.max(0)).astype(int)+1, [x1, y1])
        if (maxxy <= minxy).any():
            continue
        xa, ya = minxy
        xb, yb = maxxy
        ys, xs = slice(ya-y0, yb-y0), slice(xa-x0, xb-x0)
        relative = np.stack([xx[ys, xs]-dst[0, 0], yy[ys, xs]-dst[0, 1]], -1)
        uv = relative @ np.linalg.inv(a).T
        inside = (uv[..., 0] >= -1e-5) & (uv[..., 1] >= -1e-5) & (uv.sum(-1) <= 1+1e-5)
        sampled = uv @ b.T+src[0]
        map_x[ys, xs][inside] = sampled[..., 0][inside]
        map_y[ys, xs][inside] = sampled[..., 1][inside]
        moved[ys, xs] |= inside & (np.linalg.norm(sampled-np.stack([xx[ys, xs], yy[ys, xs]], -1), axis=-1) > .025)
    # Remap only the small ROI. Reassign only displaced pixels, so roundoff in
    # the identity mapping cannot alter the rest of the frame by one code value.
    sampled = cv2.remap(image[y0:y1, x0:x1], map_x-x0, map_y-y0,
                        cv2.INTER_CUBIC, borderMode=cv2.BORDER_REFLECT_101)
    output = image.copy()
    output[y0:y1, x0:x1][moved] = sampled[moved]
    return output, [int(x0), int(y0), int(x1), int(y1)]


def enroll_oral_reference(path: Path, annotation_path: Path | None = None) -> dict:
    """Sample strictly between curved inner lips, never a rectangular face patch."""
    image = cv2.imread(str(path))
    if image is None:
        raise ValueError("oral reference could not be decoded")
    with mp.solutions.face_mesh.FaceMesh(static_image_mode=True, max_num_faces=1,
            refine_landmarks=True, min_detection_confidence=.4) as mesh:
        contours = crop_landmarks(image, [0, 0, image.shape[1], image.shape[0]], mesh)
    if contours is None:
        raise ValueError("oral reference has no admitted face geometry")
    annotation_sha = None
    if annotation_path:
        annotation_bytes = annotation_path.read_bytes()
        annotation = json.loads(annotation_bytes.decode("utf-8-sig"))
        if annotation["sourceSha256"].lower() != digest(path):
            raise ValueError("manual oral contour source hash mismatch")
        for key in ("innerUpper", "innerLower"):
            points = np.asarray(annotation[key], np.float64)
            if (points.ndim != 2 or points.shape[1] != 2 or not 3 <= len(points) <= 64
                    or not np.isfinite(points).all() or np.any(points < 0)
                    or np.any(points[:,0] >= image.shape[1]) or np.any(points[:,1] >= image.shape[0])
                    or np.any(np.diff(points[:,0]) <= 0)):
                raise ValueError("invalid manual oral contour")
            contours[key] = points
        annotation_sha = hashlib.sha256(annotation_bytes).hexdigest()
    center, axes, width = mouth_coordinates(contours)
    local = {name: (points-center) @ axes.T for name, points in contours.items()}
    left = max(local["innerUpper"][0, 0], local["innerLower"][0, 0])
    right = min(local["innerUpper"][-1, 0], local["innerLower"][-1, 0])
    xs = np.linspace(left, right, 128)
    def curve(name):
        p = local[name]
        order = np.argsort(p[:, 0])
        return np.interp(xs, p[order, 0], p[order, 1])
    upper, lower = curve("innerUpper"), curve("innerLower")
    gap = lower-upper
    if float(gap[64]) < max(3, width*.035):
        raise ValueError("reference lacks resolved oral opening")
    # Erode in reference pixels before normalization. Invalid corner columns
    # carry zero alpha and cannot leak generated vermilion into the cavity.
    inset = np.maximum(1.25, gap*.08)
    valid = gap > 2*inset+.5
    v = np.linspace(0, 1, 64)[:, None]
    ys = upper+inset+v*np.maximum(gap-2*inset, 0)
    points = np.stack([np.broadcast_to(xs, ys.shape), ys], -1) @ axes+center
    texture = cv2.remap(image, points[..., 0].astype(np.float32),
                       points[..., 1].astype(np.float32), cv2.INTER_LINEAR)
    texture[:, ~valid] = 0
    # Context luminance is only used for bounded exposure adaptation.
    cx, cy = center.astype(int)
    rad = max(2, int(width*.75))
    context = image[max(0,cy-rad):cy+rad, max(0,cx-rad):cx+rad]
    return {"texture": texture, "alpha": np.broadcast_to(valid, ys.shape).astype(np.float32),
            "contextMean": float(np.mean(context)), "sourceSha256": digest(path),
            "manualOralContourSha256": annotation_sha,
            "mouthWidthPixels": width, "gapPixels": float(gap[64]),
            "contours": {key: value.tolist() for key, value in contours.items()}}


def warp_lip_strips(image: np.ndarray, contours: dict, aperture: float,
                    width_scale: float, strength: float,
                    oral: dict | None = None) -> tuple[np.ndarray, dict]:
    """Inverse-map ordered lip surfaces; the inner gap is a separate strip.

    Unlike unconstrained Delaunay faces, ordered vertical strips cannot swap
    upper/lower surfaces when a bilabial closes the cavity. The original skin
    is sampled throughout; no foreign texture or colour-fit is involved.
    """
    center, axes, width = mouth_coordinates(contours)
    if width < 24:
        raise ValueError("mouth below 24-pixel deformation floor")
    local = {key: (points-center) @ axes.T for key, points in contours.items()}
    radius = width*np.array([1.04, .74])
    corners = np.array([[-1,-1], [-1,1], [1,-1], [1,1]])*radius
    bounds = corners @ axes+center
    x0, y0 = np.maximum(np.floor(bounds.min(0)-2).astype(int), 0)
    x1, y1 = np.minimum(np.ceil(bounds.max(0)+2).astype(int), image.shape[1::-1])
    yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
    pos = np.stack([xx-center[0], yy-center[1]], -1) @ axes.T
    tx, ty = pos[..., 0], pos[..., 1]
    horizontal_falloff = np.clip((.85-np.abs(tx)/width)/.26, 0, 1)
    horizontal_falloff = horizontal_falloff**2*(3-2*horizontal_falloff)
    sx = tx/(1+strength*(width_scale-1)*horizontal_falloff)
    def curve(name):
        points = local[name]
        order = np.argsort(points[:, 0])
        return np.interp(sx, points[order, 0], points[order, 1])
    # CONTOURS insertion order is upper-outer, lower-outer, upper-inner,
    # lower-inner; name explicitly to avoid a topology/indexing convention.
    uo, lo, ui, li = (curve(key) for key in ("outerUpper", "outerLower", "innerUpper", "innerLower"))
    if float(np.min(li-ui)) < -.75:
        raise ValueError("crossed source inner contour")
    source_gap = np.maximum(li-ui, .12)
    seam = ui*.68+li*.32
    ui, li = seam-.32*source_gap, seam+.68*source_gap
    uo, lo = np.minimum(uo, ui-.25), np.maximum(lo, li+.25)
    center_gap = max(.12, float(local["innerLower"][5,1]-local["innerUpper"][5,1]))
    contact = aperture < .02
    if contact:
        # Articulation strength may soften a vowel, but a bilabial is a
        # topological contact event: no residual cavity is valid at any gain.
        strength = 1.0
    desired = 0.0 if contact else max(.55, width*aperture)
    cap = width*.22 if oral is not None else max(1.8, center_gap*2.25)
    shape = np.maximum(0, 1-(sx/(width*.46))**2)**.8
    target_gap = source_gap+strength*(min(desired, cap)*shape-source_gap)
    target_gap = np.maximum(target_gap, 0 if contact else .12)
    tui, tli = seam-.32*target_gap, seam+.68*target_gap
    tuo, tlo = tui-(ui-uo), tli+(lo-li)
    top = np.full_like(sx, -width*.72)
    bottom = np.full_like(sx, width*.72)
    original = [top, uo, ui, li, lo, bottom]
    target = [top, tuo, tui, tli, tlo, bottom]
    if contact:
        # Contact is occlusion of the oral surface, not compression of its
        # teeth into a bright one-pixel line. Sample within each real lip.
        original[2] = np.maximum(uo+.1, ui-.75)
        original[3] = np.minimum(lo-.1, li+.75)
    sy = ty.copy()
    for index in range(5):
        if contact and index == 2:
            continue
        ta, tb = target[index:index+2]
        sa, sb = original[index:index+2]
        if np.any(tb-ta < .01) or np.any(sb-sa < .01):
            raise ValueError("unordered lip strip")
        inside = (ty >= ta) & (ty < tb)
        ratio = np.clip((ty-ta)/np.maximum(tb-ta, .01), 0, 1)
        sy[inside] = (sa+ratio*(sb-sa))[inside]
    # Suppress exterior extrapolation and pin the entire boundary continuously.
    fade_x = np.clip((.80-np.abs(tx)/width)/.22, 0, 1)
    fade_y = np.clip((.71-np.abs(ty)/width)/.20, 0, 1)
    fade = (fade_x**2*(3-2*fade_x))*(fade_y**2*(3-2*fade_y))
    mapped = np.stack([tx+(sx-tx)*fade, ty+(sy-ty)*fade], -1) @ axes+center
    dx_dy, dx_dx = np.gradient(mapped[..., 0])
    dy_dy, dy_dx = np.gradient(mapped[..., 1])
    determinant = dx_dx*dy_dy-dx_dy*dy_dx
    # Opening a previously closed mouth necessarily exposes new area. Its
    # inverse cavity map may have very small positive area, but that cavity is
    # replaced separately. The current-pixel lip surfaces must remain regular.
    surface = ((ty < tui-1.1) | (ty > tli+1.1)) if oral is not None else np.ones(ty.shape, bool)
    if (not np.isfinite(determinant).all() or float(determinant.min()) <= 0
            or float(determinant[surface].min()) < .05):
        raise ValueError(f"inverse lip field folds: minimum {determinant.min():.5f}")
    moved = np.linalg.norm(mapped-np.stack([xx, yy], -1), axis=-1) > .025
    sample = cv2.remap(image[y0:y1, x0:x1],
                       (mapped[...,0]-x0).astype(np.float32),
                       (mapped[...,1]-y0).astype(np.float32), cv2.INTER_CUBIC,
                       borderMode=cv2.BORDER_REFLECT_101)
    result = image.copy()
    result[y0:y1, x0:x1][moved] = sample[moved]
    oral_pixels = 0
    if oral is not None and not contact:
        inner_left = max(local["innerUpper"][0,0], local["innerLower"][0,0])
        inner_right = min(local["innerUpper"][-1,0], local["innerLower"][-1,0])
        u = (sx-inner_left)/max(1, inner_right-inner_left)
        v = (ty-tui)/np.maximum(tli-tui, .01)
        distance = np.minimum(ty-tui, tli-ty)
        # Feather exclusively inward. Every nonzero reference contribution
        # stays at least .65 real game pixels inside the current lip boundary.
        alpha = np.clip((distance-.65)/.8, 0, 1)
        alpha *= ((u > 0) & (u < 1) & (v > 0) & (v < 1))
        map_u, map_v = (u*127).astype(np.float32), (v*63).astype(np.float32)
        alpha *= cv2.remap(oral["alpha"], map_u, map_v, cv2.INTER_LINEAR,
                           borderMode=cv2.BORDER_CONSTANT)
        interior = cv2.remap(oral["texture"], map_u, map_v, cv2.INTER_LINEAR,
                            borderMode=cv2.BORDER_CONSTANT).astype(np.float32)
        cx, cy = center.astype(int)
        rad = max(2, int(width*.75))
        context = image[max(0,cy-rad):cy+rad, max(0,cx-rad):cx+rad]
        gain = np.clip(float(np.mean(context))/max(1, oral["contextMean"]), .6, 1.4)
        interior = np.clip(interior*gain, 0, 255)
        region = result[y0:y1, x0:x1]
        use = alpha > 0
        blended = np.rint(region*(1-alpha[...,None])+interior*alpha[...,None]).astype(np.uint8)
        region[use] = blended[use]
        oral_pixels = int(use.sum())
    return result, {"mouthWidthPixels": width, "sourceGapPixels": center_gap,
                    "targetGapPixels": float(center_gap+strength*(min(desired, cap)-center_gap)),
                    "requestedGapPixels": float(desired),
                    "openingLimitedBySource": bool(desired > cap),
                    "minimumInverseJacobian": float(determinant.min()),
                    "minimumLipSurfaceInverseJacobian": float(determinant[surface].min()),
                    "contactOccludesCavity": bool(contact), "oralReferencePixels": oral_pixels,
                    "pixelBounds": list(map(int, [x0,y0,x1,y1]))}


def cue_file(path: Path) -> tuple[int, list[tuple[int, int, int]]]:
    lines = path.read_text().splitlines()
    header = lines[0].split()
    if len(header) != 4 or header[0] != "npc-mouth-cues-v1":
        raise ValueError("unsupported cue header")
    return int(header[1]), [tuple(map(int, line.split())) for line in lines[1:] if line]


def descriptor(category: int) -> tuple[float, float]:
    if category == 1:  # bilabial: contact
        return 0.009, 1.0
    if category in (2, 3):
        return .035, 1.025
    if category == 8:  # rounded
        return .15, .90
    if category in (7, 9):
        return .20, 1.0
    if category == 10:
        return .115, 1.06
    if category in (4, 5, 6):
        return .045, 1.025
    return .06, 1.0


def continuous_cue_trajectory(cues, rate):
    """Shape-preserving C1 trajectory with finite bilabial contact holds.

    This offline comparator deliberately knows the utterance's full cue
    schedule. It does not claim a bounded streaming lookahead. Interpolation
    moves geometry, never blends previously generated RGB frames.
    """
    knots = {}
    for start, end, category in cues:
        begin, finish = start/rate, end/rate
        aperture, width = descriptor(category)
        value = [aperture, width, 1.0 if category else 0.0]
        if category == 1:
            margin = min(.015, (finish-begin)*.25)
            value[0] = 0
            knots[begin+margin] = value
            knots[finish-margin] = value
        elif category == 0:
            knots[min(finish, begin+.050)] = value
        else:
            knots[(begin+finish)*.5] = value
    first, last = min(knots), max(knots)
    knots[0] = knots[first]
    knots[max(last+.001, cues[-1][1]/rate)] = knots[last]
    times = sorted(knots)
    return PchipInterpolator(times, [knots[t] for t in times], axis=0, extrapolate=False)


def main() -> None:
    parser = argparse.ArgumentParser(allow_abbrev=False)
    parser.add_argument("--frames", type=Path, required=True)
    parser.add_argument("--face-box", type=int, nargs=4, required=True)
    parser.add_argument("--cues", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fps", type=float, default=30)
    parser.add_argument("--strength", type=float, default=1)
    parser.add_argument("--limit", type=int, default=180)
    parser.add_argument("--warp", choices=("strips", "triangles"), default="strips")
    parser.add_argument("--oral-reference", type=Path)
    parser.add_argument("--oral-contours", type=Path)
    parser.add_argument("--manual-exclusions", type=Path)
    parser.add_argument("--trajectory", choices=("continuous", "category-ema"), default="continuous")
    args = parser.parse_args()
    if args.oral_reference and args.warp != "strips":
        raise ValueError("oral references require the strips renderer")
    if args.oral_contours and not args.oral_reference:
        raise ValueError("manual oral contours require a reference image")
    if not 0 < args.fps <= 120 or not 0 < args.strength <= 1 or not 1 <= args.limit <= 600:
        raise ValueError("invalid bounded render settings")
    output = args.output.resolve()
    if output.exists() or not str(output).lower().startswith("e:\\temp\\"):
        raise ValueError("output must be a fresh directory below E:\\temp")
    paths = sorted(p for p in args.frames.iterdir() if p.suffix.lower() in (".ppm", ".png"))[:args.limit]
    if not paths:
        raise ValueError("no input frames")
    rate, cues = cue_file(args.cues)
    trajectory = continuous_cue_trajectory(cues, rate)
    exclusions = []
    if args.manual_exclusions:
        exclusions = json.loads(args.manual_exclusions.read_text(encoding="utf-8-sig"))["intervals"]
        for entry in exclusions:
            if not 0 <= entry["firstFrame"] <= entry["lastFrame"] < len(paths) or not entry["reason"]:
                raise ValueError("invalid manual exclusion interval")
    output.mkdir(parents=True)
    (output/"frames").mkdir()
    oral = enroll_oral_reference(args.oral_reference, args.oral_contours) if args.oral_reference else None
    if oral is not None:
        cv2.imwrite(str(output/"oral-interior-only.png"), np.dstack([
            oral["texture"], np.rint(oral["alpha"]*255).astype(np.uint8)]))
        (output/"oral-reference.json").write_text(json.dumps({key: value for key, value in oral.items()
            if key not in ("texture", "alpha")}, indent=2)+"\n", encoding="utf-8")
    rows, geometry, timing = [], [], []
    triangles = None
    smooth = np.array([.06, 1.0])
    previous_shape = None
    previous_gray, previous_raw = None, None
    previous_was_bridge = False
    with mp.solutions.face_mesh.FaceMesh(static_image_mode=False, max_num_faces=1,
              refine_landmarks=True, min_detection_confidence=.5,
              min_tracking_confidence=.5) as mesh:
        for index, path in enumerate(paths):
            image = cv2.imread(str(path))
            if image is None:
                raise ValueError(f"decode failed: {path}")
            sample = int(index/args.fps*rate)
            category = next((c for start, end, c in cues if start <= sample < end), 0)
            started = time.perf_counter()
            contours = crop_landmarks(image, args.face_box, mesh)
            current_gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
            geometry_source = "mediapipe-current-frame" if contours is not None else "missing"
            if contours is None and previous_raw is not None and not previous_was_bridge:
                contours = bridge_geometry(previous_gray, current_gray, previous_raw)
                if contours is not None:
                    geometry_source = "one-frame-source-optical-flow"
            previous_raw = contours
            previous_gray = current_gray
            previous_was_bridge = geometry_source == "one-frame-source-optical-flow"
            if contours is not None:
                contours, previous_shape = smooth_local_shape(contours, previous_shape, args.fps)
            else:
                previous_shape = None
            landmark_ms = (time.perf_counter()-started)*1000
            result, reason, extra = image, "silence", {}
            started = time.perf_counter()
            wanted = np.array(descriptor(category))
            # This offline proof has a cue schedule. Explicitly use only 40ms
            # of future cues for contact preparation and release; a runtime
            # must buffer that much aligned audio before claiming parity.
            upcoming = next(((start, c) for start, end, c in cues if start > sample), None)
            if contours is not None and upcoming and upcoming[0]-sample <= rate*.04:
                ratio = 1-(upcoming[0]-sample)/(rate*.04)
                if upcoming[1] == 1:
                    wanted = wanted*(1-ratio)+np.array(descriptor(1))*ratio
                elif upcoming[1] == 0:
                    center, axes, mouth_width = mouth_coordinates(contours)
                    gap = float((contours["innerLower"][5]-contours["innerUpper"][5]) @ axes[1])
                    wanted = wanted*(1-ratio)+np.array([max(.12,gap)/mouth_width,1.0])*ratio
            # Preserve the old abrupt trajectory as a labeled regression mode.
            speech_strength = 1.0 if category else 0.0
            if args.trajectory == "continuous":
                control = trajectory(index/args.fps)
                if np.isfinite(control).all():
                    smooth = np.maximum(control[:2], [0,.7])
                    speech_strength = float(np.clip(control[2],0,1))
                else:
                    speech_strength = 0.0
            else:
                smooth = wanted if category == 1 else smooth+(wanted-smooth)*(1-math.exp(-1/(args.fps*.035)))
            excluded = next((entry for entry in exclusions if entry["firstFrame"] <= index <= entry["lastFrame"]), None)
            if excluded is not None:
                reason = "manual-visibility-bypass: "+excluded["reason"]
                previous_shape = None
                previous_raw = None
            elif speech_strength > .01 and contours is not None:
                try:
                    if args.warp == "strips":
                        result, extra = warp_lip_strips(image, contours, *smooth, args.strength*speech_strength, oral)
                    else:
                        src, dst, extra = control_points(contours, *smooth, args.strength*speech_strength)
                        if triangles is None:
                            triangles = Delaunay(src).simplices
                        result, bounds = warp_pixels(image, src, dst, triangles)
                        extra["pixelBounds"] = bounds
                    reason = "current-pixel-warp"
                except ValueError as error:
                    reason = str(error)
            elif contours is None:
                reason = "missing-face-geometry"
            render_ms = (time.perf_counter()-started)*1000
            timing.append(render_ms)
            changed = np.any(result != image, axis=2)
            rows.append({"frame": index, "category": category, "reason": reason,
                         "geometrySource": geometry_source,
                         "cueAperture": float(smooth[0]), "cueWidthScale": float(smooth[1]),
                         "speechStrength": speech_strength,
                         "changedPixels": int(changed.sum()), "landmarkMs": landmark_ms,
                         "renderMs": render_ms, **extra})
            geometry.append({"file": path.name, "contours": None if contours is None else
                             {name: points.tolist() for name, points in contours.items()}})
            cv2.imwrite(str(output/"frames"/f"frame-{index:05d}.png"), result)
            if index in (0, 8, 12, 16, 20, 36, 40, 54, 68, 76):
                x0, y0, x1, y1 = args.face_box
                panel = np.concatenate([image[y0:y1, x0:x1], result[y0:y1, x0:x1]], axis=1)
                scale = min(1040/panel.shape[1], 580/panel.shape[0])
                panel = cv2.resize(panel, (round(panel.shape[1]*scale), round(panel.shape[0]*scale)))
                cv2.imwrite(str(output/f"detail-{index:05d}.png"), panel)
                if contours is not None:
                    center, _, mouth_width = mouth_coordinates(contours)
                    cx, cy = center.astype(int)
                    radx, rady = int(mouth_width*.9), int(mouth_width*.52)
                    left, right = max(0,cx-radx), min(image.shape[1],cx+radx)
                    top, bottom = max(0,cy-rady), min(image.shape[0],cy+rady)
                    mouth = np.concatenate([image[top:bottom,left:right],result[top:bottom,left:right]],1)
                    cv2.imwrite(str(output/f"mouth-{index:05d}.png"), cv2.resize(mouth,None,fx=5,fy=5,interpolation=cv2.INTER_NEAREST))
    report = {"schema": "interactive-npcs-current-pixel-mouth-proof/v1",
              "scope": "offline manual-face-crop comparator; no game/runtime/admission proof",
              "sourceFrames": str(args.frames.resolve()), "firstSourceSha256": digest(paths[0]),
              "cueSha256": digest(args.cues), "faceBox": args.face_box, "fps": args.fps,
              "strength": args.strength, "warp": args.warp, "frameCount": len(rows),
              "cueLookaheadMs": None if args.trajectory == "continuous" else 40,
              "cueTrajectory": args.trajectory,
              "cueScheduleScope": "whole utterance known offline; no streaming parity" if args.trajectory == "continuous" else "bounded 40ms cue anticipation",
              "rendererSha256": digest(Path(__file__)),
              "runtimeVersions": {"python": platform.python_version(), "opencv": cv2.__version__,
                                  "numpy": np.__version__, "mediapipe": mp.__version__, "scipy": scipy.__version__},
              "shapeSmoothingMs": 25, "shapeSmoothingMaxDisplacementMouthWidths": .015,
              "manualExclusionsSha256": digest(args.manual_exclusions) if args.manual_exclusions else None,
              "renderP95Ms": float(np.percentile(timing, 95)),
              "renderTimingExcludes": ["landmarks", "image decode/encode", "audio", "display"],
              "oralReferenceSha256": oral["sourceSha256"] if oral else None,
              "limitations": ["Single optional generated oral reference is unverified anatomy", "Manual face crop",
                              "MediaPipe comparator geometry is not native actor admission"],
              "frames": rows}
    (output/"report.json").write_text(json.dumps(report, indent=2)+"\n", encoding="utf-8")
    (output/"geometry.json").write_text(json.dumps(geometry)+"\n", encoding="utf-8")
    print(json.dumps({"output": str(output), "frames": len(rows),
                      "warped": sum(row["reason"] == "current-pixel-warp" for row in rows),
                      "renderP95Ms": report["renderP95Ms"]}))


if __name__ == "__main__":
    main()
