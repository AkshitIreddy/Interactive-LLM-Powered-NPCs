"""Frozen identities and hard protocol ceilings for the local LLM worker."""

from __future__ import annotations

PROTOCOL_VERSION = "1.0"
PACK_SCHEMA = "npc.model-pack/v2"
RUNTIME_BUNDLE_SCHEMA = "npc.local-llm.runtime-bundle/v1"
MEASUREMENT_SCHEMA = "npc.local-llm.measurement/v1"

MODEL_ID = "qwen3-4b-instruct-2507-q4-k-m"
MODEL_REVISION = "Qwen3-4B-Instruct-2507"
MODEL_ARTIFACT_SIZE = 2_497_281_120
MODEL_ARTIFACT_SHA256 = "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597"
RUNTIME_RELEASE = "b10689"
RUNTIME_COMMIT = "57291f2644af8c9df0dd8d44395881c5bdcf0ecd"
RUNTIME_ABI = "npc-llama-server-openai-v1+b10689+qwen3"

MAX_FRAME_BYTES = 1_048_576
MAX_TEXT_BYTES = 262_144
MAX_MESSAGES = 64
MAX_MESSAGE_BYTES = 65_536
MAX_SYSTEM_BYTES = 32_768
MAX_RESPONSE_SCHEMA_BYTES = 65_536
MAX_OUTPUT_TOKENS = 2_048
MAX_CONTEXT_TOKENS = 32_768
MAX_EVENTS_PER_REQUEST = 4_096
MAX_HTTP_ERROR_BYTES = 8_192
MAX_SSE_EVENT_BYTES = 1_048_576
MAX_DOWNLOAD_ARTIFACTS = 16
MAX_ARCHIVE_MEMBERS = 512
MAX_ARCHIVE_EXPANDED_BYTES = 512 * 1_048_576
ABSOLUTE_MAX_ARCHIVE_EXPANDED_BYTES = 2 * 1_073_741_824
MAX_ARCHIVE_EXPANSION_RATIO = 200

ALLOWED_ROLES = frozenset({"system", "user", "assistant"})
ALLOWED_OPERATIONS = frozenset(
    {"handshake", "capabilities", "health", "warm", "load", "unload", "infer", "cancel", "shutdown"}
)
ALLOWED_DOWNLOAD_HOSTS = frozenset(
    {
        "huggingface.co",
        "cdn-lfs.huggingface.co",
        "cas-bridge.xethub.hf.co",
        "us.aws.cdn.hf.co",
        "github.com",
        "release-assets.githubusercontent.com",
        "objects.githubusercontent.com",
    }
)
