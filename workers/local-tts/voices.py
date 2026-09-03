"""Audited Kokoro v1.0 English stock-voice allowlist.

The speaker IDs are the immutable order used by k2-fsa's v1.0
``generate_voices_bin.py``.  Only American and British English voices are
exposed by this pack.  The archive contains other embeddings, but they are not
selectable until their language path and attribution are separately qualified.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class StockVoice:
    voice_id: str
    speaker_id: int
    display_name: str
    language: str
    locale: str
    upstream_embedding_sha256: str
    upstream_quality_grade: str
    license_id: str = "kokoro-model-apache-2.0"
    is_voice_clone: bool = False


_VOICE_ROWS = (
    ("af_alloy", 0, "Alloy", "en-US", "6d877149dd8b348fbad12e5845b7e43d975390e9f3b68a811d1d86168bef5aa3", "C"),
    ("af_aoede", 1, "Aoede", "en-US", "c03bd1a4c3716c2d8eaa3d50022f62d5c31cfbd6e15933a00b17fefe13841cc4", "C+"),
    ("af_bella", 2, "Bella", "en-US", "8cb64e02fcc8de0327a8e13817e49c76c945ecf0052ceac97d3081480e8e48d6", "A-"),
    ("af_heart", 3, "Heart", "en-US", "0ab5709b8ffab19bfd849cd11d98f75b60af7733253ad0d67b12382a102cb4ff", "A"),
    ("af_jessica", 4, "Jessica", "en-US", "cdfdccb8cc975aa34ee6b89642963b0064237675de0e41a30ae64cc958dd4e87", "D"),
    ("af_kore", 5, "Kore", "en-US", "8bfbc512321c3db49dff984ac675fa5ac7eaed5a96cc31104d3a9080e179d69d", "C+"),
    ("af_nicole", 6, "Nicole", "en-US", "c5561808bcf5250fe8c5f5de32caf2d94f27e57e95befdb098c5c85991d4c5da", "B-"),
    ("af_nova", 7, "Nova", "en-US", "e0233676ddc21908c37a1f102f6b88a59e4e5c1bd764983616eb9eda629dbcd2", "C"),
    ("af_river", 8, "River", "en-US", "e149459bd9c084416b74756b9bd3418256a8b839088abb07d463730c369dab8f", "D"),
    ("af_sarah", 9, "Sarah", "en-US", "49bd364ea3be9eb3e9685e8f9a15448c4883112a7c0ff7ab139fa4088b08cef9", "C+"),
    ("af_sky", 10, "Sky", "en-US", "c799548aed06e0cb0d655a85a01b48e7f10484d71663f9a3045a5b9362e8512c", "C-"),
    ("am_adam", 11, "Adam", "en-US", "ced7e284aba12472891be1da3ab34db84cc05cc02b5889535796dbf2d8b0cb34", "F+"),
    ("am_echo", 12, "Echo", "en-US", "8bcfdc852bc985fb45c396c561e571ffb9183930071f962f1b50df5c97b161e8", "D"),
    ("am_eric", 13, "Eric", "en-US", "ada66f0eefff34ec921b1d7474d7ac8bec00cd863c170f1c534916e9b8212aae", "D"),
    ("am_fenrir", 14, "Fenrir", "en-US", "98e507eca1db08230ae3b6232d59c10aec9630022d19accac4f5d12fcec3c37a", "C+"),
    ("am_liam", 15, "Liam", "en-US", "c82550757ddb31308b97f30040dda8c2d609a9e2de6135848d0a948368138518", "D"),
    ("am_michael", 16, "Michael", "en-US", "9a443b79a4b22489a5b0ab7c651a0bcd1a30bef675c28333f06971abbd47bd37", "C+"),
    ("am_onyx", 17, "Onyx", "en-US", "e8452be16cd0f6da7b4579eaf7b1e4506e92524882053d86d72b96b9a7fed584", "D"),
    ("am_puck", 18, "Puck", "en-US", "dd1d8973f4ce4b7d8ae407c77a435f485dabc052081b80ea75c4f30b84f36223", "C+"),
    ("am_santa", 19, "Santa", "en-US", "7f2f7582fa2b1f160e90aafe6d0b442a685e773608b6667e545d743b073e97a7", "D-"),
    ("bf_alice", 20, "Alice", "en-GB", "d292651b6af6c0d81705c2580dcb4463fccc0ff7b8d618a471dbb4e45655b3f3", "D"),
    ("bf_emma", 21, "Emma", "en-GB", "d0a423deabf4a52b4f49318c51742c54e21bb89bbbe9a12141e7758ddb5da701", "B-"),
    ("bf_isabella", 22, "Isabella", "en-GB", "cdd4c37003805104d1d08fb1e05855c8fb2c68de24ca6e71f264a30aaa59eefd", "C"),
    ("bf_lily", 23, "Lily", "en-GB", "6e09c2e481e2d53004d7e5ae7d3a325369e130a6f45c35a6002de75084be9285", "D"),
    ("bm_daniel", 24, "Daniel", "en-GB", "fc3fce4e9c12ed4dbc8fa9680cfe51ee190a96444ce7c3ad647549a30823fc5d", "D"),
    ("bm_fable", 25, "Fable", "en-GB", "d44935f3135257a9064df99f007fc1342ff1aa767552b4a4fa4c3b2e6e59079c", "C"),
    ("bm_george", 26, "George", "en-GB", "f1bc812213dc59774769e5c80004b13eeb79bd78130b11b2d7f934542dab811b", "C"),
    ("bm_lewis", 27, "Lewis", "en-GB", "b5204750dcba01029d2ac9cec17aec3b20a6d64073c579d694a23cb40effbd0e", "D+"),
)

STOCK_VOICES = tuple(
    StockVoice(
        voice_id=voice_id,
        speaker_id=speaker_id,
        display_name=display_name,
        language="English",
        locale=locale,
        upstream_embedding_sha256=digest,
        upstream_quality_grade=grade,
    )
    for voice_id, speaker_id, display_name, locale, digest, grade in _VOICE_ROWS
)

VOICE_BY_ID = {voice.voice_id: voice for voice in STOCK_VOICES}


def public_voice_records() -> list[dict[str, object]]:
    return [
        {
            "voice_id": voice.voice_id,
            "speaker_id": voice.speaker_id,
            "display_name": voice.display_name,
            "language": voice.language,
            "locale": voice.locale,
            "quality_grade": voice.upstream_quality_grade,
            "license_id": voice.license_id,
            "stock_voice": True,
            "voice_cloning": False,
        }
        for voice in STOCK_VOICES
    ]
