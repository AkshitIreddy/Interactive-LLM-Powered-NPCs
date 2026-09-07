#!/usr/bin/env python3
"""Synthetic pixel-contract tests for the offline mouth-warp experiment.

These tests deliberately avoid real faces and quality scores.  The source uses
coordinate-varying skin, separately coloured upper/lower lips, and a unique
oral-cavity colour.  The optional oral reference uses a colour that cannot
occur in the source.  That makes pixel provenance and contact occlusion
testable without treating the experiment as production evidence.
"""
from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest

import cv2
import numpy as np


_RENDERER_PATH = Path(__file__).with_name("render-current-pixel-mouth-proof.py")
_SPEC = importlib.util.spec_from_file_location("current_pixel_mouth_proof", _RENDERER_PATH)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError(f"cannot import mouth proof from {_RENDERER_PATH}")
mouth_proof = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(mouth_proof)


class CurrentPixelMouthProofTests(unittest.TestCase):
    frame_height = 144
    frame_width = 192
    mouth_center = np.array([96.0, 72.0])

    @classmethod
    def contours(cls, width: float = 60.0) -> dict[str, np.ndarray]:
        """Return ordered, closed-at-corners synthetic lip contours."""
        x = np.linspace(cls.mouth_center[0] - width / 2,
                        cls.mouth_center[0] + width / 2, 11)
        normalized_x = (x - cls.mouth_center[0]) / (width / 2)
        arch = np.maximum(0.0, 1.0 - normalized_x * normalized_x)
        center_y = cls.mouth_center[1]
        return {
            "outerUpper": np.column_stack((x, center_y - 7.0 * arch)),
            "outerLower": np.column_stack((x, center_y + 7.0 * arch)),
            "innerUpper": np.column_stack((x, center_y - 2.4 * arch)),
            "innerLower": np.column_stack((x, center_y + 2.4 * arch)),
        }

    @classmethod
    def source_frame(cls, contours: dict[str, np.ndarray]) -> np.ndarray:
        """Create coordinate skin plus source-owned lips and cavity sentinels."""
        yy, xx = np.indices((cls.frame_height, cls.frame_width))
        image = np.empty((cls.frame_height, cls.frame_width, 3), np.uint8)
        image[..., 0] = 20 + xx % 70
        image[..., 1] = 35 + yy % 70
        image[..., 2] = 45 + (3 * xx + 5 * yy) % 70

        def polygon(*parts: np.ndarray) -> np.ndarray:
            return np.rint(np.concatenate(parts)).astype(np.int32)

        # BGR sentinels: red upper lip, blue lower lip, magenta cavity.
        cv2.fillPoly(image, [polygon(contours["outerUpper"],
                                             contours["innerUpper"][::-1])],
                     (30, 40, 220))
        cv2.fillPoly(image, [polygon(contours["outerLower"],
                                             contours["innerLower"][::-1])],
                     (220, 40, 30))
        cv2.fillPoly(image, [polygon(contours["innerUpper"],
                                             contours["innerLower"][::-1])],
                     (251, 7, 251))
        return image

    @staticmethod
    def oral_reference(image: np.ndarray, contours: dict[str, np.ndarray],
                       alpha: float = 1.0) -> dict[str, np.ndarray | float]:
        center, _, width = mouth_proof.mouth_coordinates(contours)
        cx, cy = center.astype(int)
        radius = max(2, int(width * .75))
        context = image[max(0, cy-radius):cy+radius,
                        max(0, cx-radius):cx+radius]
        return {
            # Pure green is impossible in the synthetic source.
            "texture": np.full((64, 128, 3), (0, 255, 0), np.uint8),
            "alpha": np.full((64, 128), alpha, np.float32),
            "contextMean": float(np.mean(context)),
        }

    @staticmethod
    def expected_eroded_aperture(image: np.ndarray,
                                 contours: dict[str, np.ndarray],
                                 aperture: float,
                                 width_scale: float = 1.0,
                                 strength: float = 1.0) -> np.ndarray:
        """Independently reconstruct the oral insert's documented support."""
        center, axes, width = mouth_proof.mouth_coordinates(contours)
        local = {name: (points-center) @ axes.T for name, points in contours.items()}
        radius = width * np.array([1.04, .74])
        corners = np.array([[-1, -1], [-1, 1], [1, -1], [1, 1]]) * radius
        bounds = corners @ axes + center
        x0, y0 = np.maximum(np.floor(bounds.min(0)-2).astype(int), 0)
        x1, y1 = np.minimum(np.ceil(bounds.max(0)+2).astype(int), image.shape[1::-1])
        yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
        position = np.stack([xx-center[0], yy-center[1]], -1) @ axes.T
        tx, ty = position[..., 0], position[..., 1]
        horizontal_falloff = np.clip((.85-np.abs(tx)/width)/.26, 0, 1)
        horizontal_falloff = horizontal_falloff**2 * (3-2*horizontal_falloff)
        sx = tx / (1 + strength*(width_scale-1)*horizontal_falloff)

        def curve(name: str) -> np.ndarray:
            points = local[name]
            order = np.argsort(points[:, 0])
            return np.interp(sx, points[order, 0], points[order, 1])

        upper = curve("innerUpper")
        lower = curve("innerLower")
        source_gap = np.maximum(lower-upper, .12)
        seam = upper*.68 + lower*.32
        desired = max(.55, width*aperture)
        target_gap = source_gap + strength * (
            min(desired, width*.22) *
            np.maximum(0, 1-(sx/(width*.46))**2)**.8 - source_gap)
        target_gap = np.maximum(target_gap, .12)
        target_upper = seam - .32*target_gap
        target_lower = seam + .68*target_gap
        inner_left = max(local["innerUpper"][0, 0], local["innerLower"][0, 0])
        inner_right = min(local["innerUpper"][-1, 0], local["innerLower"][-1, 0])
        u = (sx-inner_left) / max(1, inner_right-inner_left)
        v = (ty-target_upper) / np.maximum(target_lower-target_upper, .01)
        distance = np.minimum(ty-target_upper, target_lower-ty)
        local_support = ((distance > .65) & (u > 0) & (u < 1) &
                         (v > 0) & (v < 1))
        support = np.zeros(image.shape[:2], bool)
        support[y0:y1, x0:x1] = local_support
        return support

    def test_oral_reference_affects_only_eroded_inner_aperture(self) -> None:
        contours = self.contours()
        source = self.source_frame(contours)
        visible_oral = self.oral_reference(source, contours, alpha=1.0)
        transparent_oral = self.oral_reference(source, contours, alpha=0.0)

        with_oral, evidence = mouth_proof.warp_lip_strips(
            source, contours, .18, 1.0, 1.0, visible_oral)
        source_only, _ = mouth_proof.warp_lip_strips(
            source, contours, .18, 1.0, 1.0, transparent_oral)
        reference_effect = np.any(with_oral != source_only, axis=2)
        admitted_support = self.expected_eroded_aperture(source, contours, .18)

        self.assertGreater(evidence["oralReferencePixels"], 0)
        self.assertGreater(int(reference_effect.sum()), 0)
        self.assertFalse(np.any(reference_effect & ~admitted_support),
                         "foreign reference pixels escaped the eroded inner aperture")
        # The source-owned upper/lower surfaces remain visibly distinct after
        # the reference is inserted; the reference cannot repaint the annulus.
        central = with_oral[:, 84:109]
        self.assertTrue(np.any((central[..., 2] > 170) &
                               (central[..., 2] > central[..., 0] * 1.5)))
        self.assertTrue(np.any((central[..., 0] > 170) &
                               (central[..., 0] > central[..., 2] * 1.5)))

    def test_contact_occludes_cavity_and_ignores_oral_reference(self) -> None:
        contours = self.contours()
        source = self.source_frame(contours)
        oral = self.oral_reference(source, contours)

        output, evidence = mouth_proof.warp_lip_strips(
            source, contours, .009, 1.0, 1.0, oral)
        center_x, center_y = self.mouth_center.astype(int)
        contact_band = output[center_y-3:center_y+4,
                              center_x-15:center_x+16]
        squeezed_cavity = ((contact_band[..., 0] > 180) &
                           (contact_band[..., 2] > 180))

        self.assertTrue(evidence["contactOccludesCavity"])
        self.assertEqual(evidence["targetGapPixels"], 0.0)
        self.assertEqual(evidence["oralReferencePixels"], 0)
        self.assertFalse(np.any(squeezed_cavity),
                         "source cavity colour survived as a squeezed contact seam")
        self.assertFalse(np.any((output[..., 1] > 200) &
                                (output[..., 0] < 40) &
                                (output[..., 2] < 40)),
                         "oral reference contributed during physical contact")

    def test_contact_remains_complete_when_visual_strength_is_reduced(self) -> None:
        contours = self.contours()
        source = self.source_frame(contours)
        output, evidence = mouth_proof.warp_lip_strips(
            source, contours, .009, 1.0, .35,
            self.oral_reference(source, contours))
        center_x, center_y = self.mouth_center.astype(int)
        contact_band = output[center_y-3:center_y+4,
                              center_x-15:center_x+16]
        squeezed_cavity = ((contact_band[..., 0] > 180) &
                           (contact_band[..., 2] > 180))

        self.assertTrue(evidence["contactOccludesCavity"])
        self.assertEqual(evidence["targetGapPixels"], 0.0)
        self.assertEqual(evidence["oralReferencePixels"], 0)
        self.assertFalse(np.any(squeezed_cavity),
                         "visual strength weakened the bilabial topology")

    def test_source_and_pixels_outside_reported_roi_remain_exact(self) -> None:
        contours = self.contours()
        source = self.source_frame(contours)
        before = source.copy()
        output, evidence = mouth_proof.warp_lip_strips(
            source, contours, .18, .90, 1.0,
            self.oral_reference(source, contours))
        x0, y0, x1, y1 = evidence["pixelBounds"]
        outside = np.ones(source.shape[:2], bool)
        outside[y0:y1, x0:x1] = False

        self.assertTrue(np.array_equal(source, before),
                        "renderer mutated the untouched input frame")
        self.assertTrue(np.array_equal(output[outside], source[outside]),
                        "renderer changed pixels outside its reported ROI")

    def test_crossed_inner_lips_are_rejected(self) -> None:
        contours = self.contours()
        contours["innerLower"][:, 1] = contours["innerUpper"][:, 1] - 2.0
        source = self.source_frame(contours)
        with self.assertRaisesRegex(ValueError, "crossed source inner contour"):
            mouth_proof.warp_lip_strips(source, contours, .12, 1.0, 1.0)

    def test_small_mouth_is_rejected_before_resampling(self) -> None:
        contours = self.contours(width=20.0)
        source = self.source_frame(contours)
        with self.assertRaisesRegex(ValueError, "mouth below 24-pixel deformation floor"):
            mouth_proof.warp_lip_strips(source, contours, .12, 1.0, 1.0)

    def test_one_frame_flow_bridge_follows_known_source_translation(self) -> None:
        contours = self.contours()
        source = cv2.cvtColor(self.source_frame(contours), cv2.COLOR_BGR2GRAY)
        translated = cv2.warpAffine(source, np.float32([[1,0,3],[0,1,2]]),
                                    source.shape[1::-1])
        bridged = mouth_proof.bridge_geometry(source, translated, contours)
        self.assertIsNotNone(bridged)
        for key, original in contours.items():
            self.assertLess(float(np.max(np.linalg.norm(bridged[key]-original-[3,2], axis=1))), .4)

    def test_flow_bridge_rejects_untextured_source(self) -> None:
        blank = np.zeros((self.frame_height,self.frame_width), np.uint8)
        self.assertIsNone(mouth_proof.bridge_geometry(blank, blank, self.contours()))

    def test_flow_bridge_rejects_a_scene_cut(self) -> None:
        contours = self.contours()
        source = cv2.cvtColor(self.source_frame(contours),cv2.COLOR_BGR2GRAY)
        unrelated = np.random.default_rng(501).integers(0,256,source.shape,dtype=np.uint8)
        self.assertIsNone(mouth_proof.bridge_geometry(source,unrelated,contours))

    def test_source_edges_reject_a_mesh_cavity_drawn_through_closed_lips(self) -> None:
        prior = self.contours()
        x = prior["outerUpper"][:,0]
        arch = np.maximum(0,1-((x-self.mouth_center[0])/30)**2)
        for key,offset in (("outerUpper",-12),("outerLower",8),("innerUpper",-8),("innerLower",2)):
            prior[key][:,1] = self.mouth_center[1]+offset*arch
        source = np.full((self.frame_height,self.frame_width,3),150,np.uint8)
        upper = np.stack([x,self.mouth_center[1]-4*arch],-1)
        lower = np.stack([x,self.mouth_center[1]+5*arch],-1)
        polygon = np.rint(np.concatenate([upper,lower[::-1]])).astype(np.int32)
        cv2.fillPoly(source,[polygon],(25,15,40))
        cv2.line(source,(72,72),(120,72),(10,5,15),1)
        corrected,evidence = mouth_proof.refine_source_edges(source,prior)
        self.assertTrue(evidence["sourceEdgeContact"])
        self.assertTrue(np.allclose(corrected["innerUpper"],corrected["innerLower"]))
        self.assertLess(abs(corrected["outerUpper"][5,1]-68),1.5)
        self.assertLess(abs(corrected["outerLower"][5,1]-77),1.5)

    def test_source_edge_refinement_rejects_flat_appearance(self) -> None:
        blank = np.full((self.frame_height,self.frame_width,3),80,np.uint8)
        with self.assertRaisesRegex(ValueError,"lack usable contrast"):
            mouth_proof.refine_source_edges(blank,self.contours())

    def test_continuous_cues_hold_contact_without_overshooting(self) -> None:
        cues = [(0,200,8),(200,300,1),(300,500,10),(500,700,0)]
        trajectory = mouth_proof.continuous_cue_trajectory(cues,1000)
        values = trajectory(np.linspace(0,.7,701))
        self.assertTrue(np.isfinite(values).all())
        self.assertGreaterEqual(float(values[:,0].min()),0)
        self.assertLessEqual(float(values[:,0].max()),.15)
        self.assertTrue(np.allclose(trajectory(np.linspace(.22,.28,7))[:,0],0))
        # A 1ms step cannot reproduce the old category discontinuity.
        self.assertLess(float(np.max(np.abs(np.diff(values[:,0])))),.005)


if __name__ == "__main__":
    unittest.main()
