# Procedural mouth atlas

Builds a private schema-4 `normalized-oral-strip-v1` mouth pack for the fictional
Eclipse Harbor character Mara Venn, so the review build can show a visibly open
mouth in the practice game.

The oral interiors (cavity, teeth band, tongue) are rendered procedurally with
numpy/OpenCV. Nothing is photographed or produced by an image model. The lips
themselves always come from the current game frame through the native compositor.

```
python tools/mouth-atlas/make_strips.py            # writes strips_preview.png
python tools/mouth-atlas/write_atlas.py --out <dir>/review-mouth-atlas \
    --evidence-sha256 <sha256 of the review sheet> --provenance <provenance note>
```

States: closed (transparent, m/b/p), "ah" open, "oo" rounded, "ee" spread, and a
small-aperture state for f/v and t/d/s. `write_atlas.py` needs `opencv-python`
and `numpy`, and refuses to overwrite an existing output folder.

Install the pack into a staged review app with
`tools/live-review/stage-review.ps1 -MouthAtlas <dir>/review-mouth-atlas`. Debug
builds load it from `review-mouth-atlas` next to the control executable. The pack
is for private review only: natural motion and ordinary targets stay unqualified.
