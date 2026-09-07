from __future__ import annotations

import base64
import json
import struct
import tempfile
import unittest
import wave
from io import BytesIO
from pathlib import Path

import provider_timing_alternatives as timing


class CredentialTests(unittest.TestCase):
    def test_reads_only_explicit_provider_labels(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "keys.txt"
            path.write_text("Deepgram = dg-secret\nInworld:\nbase64-secret\nunknown=ignored\n", encoding="utf-8")
            self.assertEqual(
                timing.read_labeled_credentials(path),
                {"deepgram": "dg-secret", "inworld": "base64-secret"},
            )

    def test_commented_provider_labels_cannot_capture_following_text(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "keys.txt"
            path.write_text(
                "# Cartesia\nnot-a-key-value\n; Deepgram = hidden\n// Inworld\nignored-value\n",
                encoding="utf-8",
            )
            self.assertEqual(timing.read_labeled_credentials(path), {})


class DecoderTests(unittest.TestCase):
    def test_raw_pcm_decoder_rejects_an_odd_terminal_byte(self) -> None:
        decoder = timing.RawPcmDecoder()
        self.assertEqual(decoder.feed(b"\x01\x00\x02"), b"\x01\x00")
        with self.assertRaisesRegex(ValueError, "odd_length_pcm"):
            decoder.finish()

    def test_wav_decoder_emits_only_pcm(self) -> None:
        pcm = struct.pack("<4h", 0, 100, -100, 200)
        output = BytesIO()
        with wave.open(output, "wb") as stream:
            stream.setnchannels(1)
            stream.setsampwidth(2)
            stream.setframerate(24_000)
            stream.writeframes(pcm)
        encoded = output.getvalue()
        decoder = timing.WavPcmDecoder()
        actual = decoder.feed(encoded[:17]) + decoder.feed(encoded[17:]) + decoder.finish()
        self.assertEqual(actual, pcm)
        self.assertEqual(decoder.metadata["sampleRate"], 24_000)

    def test_sse_decoder_extracts_audio_and_alignment_summary(self) -> None:
        pcm = struct.pack("<3h", 0, 50, -50)
        event = {
            "result": {
                "audioContent": base64.b64encode(pcm).decode(),
                "timestampInfo": {
                    "wordAlignment": {
                        "phoneticDetails": [
                            {"phones": [{"phoneSymbol": "m", "visemeSymbol": "bmp"}]}
                        ]
                    }
                },
            }
        }
        payload = f"data: {json.dumps(event)}\n\n".encode()
        decoder = timing.JsonAudioDecoder()
        actual = decoder.feed(payload[:9]) + decoder.feed(payload[9:]) + decoder.finish()
        self.assertEqual(actual, pcm)
        self.assertEqual(decoder.metadata["phonemeCount"], 1)
        self.assertEqual(decoder.metadata["visemeCount"], 1)
        self.assertEqual(decoder.metadata["audioEvents"], 1)
        self.assertEqual(decoder.metadata["smallestAudioEventBytes"], len(pcm))
        self.assertEqual(decoder.metadata["largestAudioEventBytes"], len(pcm))

    def test_audio_metrics_detects_signal(self) -> None:
        pcm = struct.pack("<4h", 0, 1000, -1000, 500)
        metrics = timing.audio_metrics(pcm, 24_000)
        self.assertFalse(metrics["silent"])
        self.assertEqual(metrics["pcmBytes"], len(pcm))
        self.assertGreater(metrics["rms"], 0)


class RequestTests(unittest.TestCase):
    def test_credentials_are_only_in_matching_official_headers(self) -> None:
        for profile in timing.PROFILES:
            spec = timing.request_spec(profile, "secret-value")
            self.assertNotIn("secret-value", spec.path)
            self.assertNotIn("secret-value", spec.body.decode("utf-8"))
            self.assertIn(
                spec.host,
                {
                    "api.openai.com",
                    "generativelanguage.googleapis.com",
                    "api.groq.com",
                    "api.cartesia.ai",
                    "api.deepgram.com",
                    "api.inworld.ai",
                    "api.elevenlabs.io",
                },
            )
            self.assertTrue(any("secret-value" in value for value in spec.headers.values()))


class ArtifactTests(unittest.TestCase):
    def test_write_new_bytes_refuses_to_replace_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receipt.json"
            timing.write_new_bytes(path, b"first")
            with self.assertRaises(FileExistsError):
                timing.write_new_bytes(path, b"second")
            self.assertEqual(path.read_bytes(), b"first")


if __name__ == "__main__":
    unittest.main()
