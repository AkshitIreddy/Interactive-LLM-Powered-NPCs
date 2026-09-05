"""Bind offline Rhubarb recognition cues to an exact WAV for native A/B review.

Recognition is a comparator, not a product streaming dependency. No character
alignment fragment is claimed to be a phoneme. See DanielSWolf/rhubarb-lip-sync.
"""
from __future__ import annotations
import argparse
from decimal import Decimal, ROUND_HALF_UP
import hashlib
import json
from pathlib import Path
import wave

# Rhubarb's cartoon vocabulary is coarser than phonemes. Map only the visible
# distinctions it actually exposes; H is tongue/alveolar, not a new phoneme.
SHAPES = {"X": 0, "A": 1, "B": 4, "C": 10, "D": 9, "E": 8, "F": 8, "G": 2, "H": 4}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--audio", required=True, type=Path)
    parser.add_argument("--rhubarb-json", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    with wave.open(str(args.audio), "rb") as wav:
        rate, count = wav.getframerate(), wav.getnframes()
        if wav.getsampwidth() != 2 or wav.getnchannels() not in (1, 2):
            raise ValueError("native proof requires PCM16 mono/stereo")
    data = json.loads(args.rhubarb_json.read_text(encoding="utf-8-sig"))
    # Rhubarb truncates its duration to centiseconds; only the final <10 ms
    # may be extended to match the precise sample count.
    duration = Decimal(str(data["metadata"]["duration"]))
    if not 0 <= Decimal(count) / rate - duration < Decimal("0.01"):
        raise ValueError("recognizer duration does not match audio")
    audio_ref = Path(data["metadata"]["soundFile"]).resolve()
    if audio_ref != args.audio.resolve():
        raise ValueError("recognizer references a different audio file")
    rows = []
    for cue in data["mouthCues"]:
        first, last = (int((Decimal(str(cue[key])) * rate).to_integral_value(rounding=ROUND_HALF_UP)) for key in ("start", "end"))
        if first != (rows[-1][1] if rows else 0) or last <= first or last > count:
            raise ValueError("non-contiguous or invalid cue interval")
        rows.append([first, last, SHAPES[cue["value"]]])
    if not rows or count - rows[-1][1] >= rate / 100:
        raise ValueError("incomplete recognized utterance")
    rows[-1][1] = count
    digest = hashlib.sha256(args.audio.read_bytes()).hexdigest()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(f"npc-mouth-cues-v1 {rate} {count} {digest}\n" +
                        "".join(f"{start} {end} {shape}\n" for start, end, shape in rows), encoding="utf-8")
    print(json.dumps({"cues": len(rows), "distinct_native_visemes": len({row[2] for row in rows}), "audio_sha256": digest, "scope": "offline recognition comparator"}))


if __name__ == "__main__":
    main()
