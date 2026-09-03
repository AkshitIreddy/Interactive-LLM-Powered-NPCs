"""Isolated production local-embedding worker lane.

The directory is intentionally not imported by the main application yet.  The
Model Manager owns installation and the runtime supervisor owns launch.  This
package keeps the model-facing implementation independently testable until
those integration seams are reviewed.
"""

