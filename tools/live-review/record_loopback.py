"""Record what the default speaker is playing (WASAPI loopback) and summarise it.

Usage:
  python -I record_loopback.py --seconds 12 --out turn.wav

Prints RMS/peak over time and the dominant frequency of loud windows, so a
Testing-voice turn (440->660 Hz tone sweep) can be confirmed without anyone
listening.
"""

from __future__ import annotations

import argparse
import wave

import numpy as np
import soundcard as sc


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seconds", type=float, default=10.0)
    parser.add_argument("--out", required=True)
    parser.add_argument("--rate", type=int, default=48000)
    args = parser.parse_args()

    speaker = sc.default_speaker()
    loopback = sc.get_microphone(id=str(speaker.name), include_loopback=True)
    frames = int(args.seconds * args.rate)
    with loopback.recorder(samplerate=args.rate, channels=2) as recorder:
        data = recorder.record(numframes=frames)
    mono = data.mean(axis=1)

    pcm = np.clip(data, -1.0, 1.0)
    with wave.open(args.out, "wb") as handle:
        handle.setnchannels(2)
        handle.setsampwidth(2)
        handle.setframerate(args.rate)
        handle.writeframes((pcm * 32767).astype("<i2").tobytes())

    window = args.rate // 10
    loud = []
    tone_windows = 0
    for start in range(0, len(mono) - window, window):
        chunk = mono[start:start + window]
        rms = float(np.sqrt(np.mean(chunk ** 2)))
        if rms > 0.003:
            spectrum = np.abs(np.fft.rfft(chunk * np.hanning(len(chunk)))) ** 2
            freqs = np.fft.rfftfreq(len(chunk), 1 / args.rate)
            peak_hz = float(freqs[int(np.argmax(spectrum))])
            # The Testing voice sweeps 440->660 Hz; other playback (music, the
            # game) lands elsewhere, so report the share of energy in that band.
            band = float(spectrum[(freqs >= 400) & (freqs <= 700)].sum() / max(spectrum.sum(), 1e-12))
            tone_windows += band > 0.5
            loud.append((start / args.rate, rms, peak_hz, band))
    print(f"speaker: {speaker.name}")
    print(f"overall peak={float(np.max(np.abs(mono))):.4f} rms={float(np.sqrt(np.mean(mono ** 2))):.5f}")
    print(f"audible 100 ms windows: {len(loud)}; testing-tone windows (400-700 Hz > 50%): {tone_windows}")
    for at, rms, hz, band in loud[:80]:
        marker = "  <- tone" if band > 0.5 else ""
        print(f"  t={at:6.2f}s rms={rms:.4f} dominant={hz:7.1f} Hz band={band:.2f}{marker}")

if __name__ == "__main__":
    main()
