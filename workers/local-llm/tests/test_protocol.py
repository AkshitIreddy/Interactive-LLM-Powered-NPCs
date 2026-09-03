from __future__ import annotations

import io
import json
import struct
import threading
import time
import unittest

from npc_local_llm.errors import LocalLlmError
from npc_local_llm.framing import FrameIO
from npc_local_llm.manifest import ModelPack, RuntimeBundle
from npc_local_llm.protocol import Request, WorkerController, parse_chat_request
from npc_local_llm.sse import decode_chunks

from support import FakeTransport, MANIFEST, RUNTIME_BUNDLE


def request(sequence: int, operation: str, payload=None, generation: int = 0):
    return {
        "protocol_version": "1.0",
        "worker_instance_id": "worker-qwen3-test",
        "request_id": f"request-{sequence}",
        "sequence": sequence,
        "generation": generation,
        "deadline_unix_ms": 0,
        "operation": operation,
        "payload": payload or {},
    }


class ProtocolTests(unittest.TestCase):
    def test_request_and_chat_payload_are_bounded(self) -> None:
        parsed = Request.parse(request(1, "handshake", {"launch_nonce": "nonce"}))
        self.assertEqual(parsed.operation, "handshake")
        chat = parse_chat_request({"prompt": "Hello", "max_tokens": 16})
        self.assertEqual(chat.messages[0]["content"], "Hello")
        with self.assertRaises(LocalLlmError):
            parse_chat_request({"prompt": "Hello", "unknown": True})
        with self.assertRaises(LocalLlmError):
            parse_chat_request(
                {
                    "prompt": "Hello",
                    "response_json_schema": {"$ref": "https://attacker.invalid/schema.json"},
                }
            )

    def test_fragmented_sse_stream_decodes(self) -> None:
        body = (
            b'data: {"choices":[{"delta":{"content":"Re"},"finish_reason":null}]}\n\n'
            b'data: {"choices":[{"delta":{"content":"ady"},"finish_reason":"stop"}],"usage":{"completion_tokens":2}}\n\n'
            b'data: [DONE]\n\n'
        )
        deltas = decode_chunks((body[:17], body[17:73], body[73:]))
        self.assertEqual("".join(delta.text for delta in deltas), "Ready")
        self.assertEqual(deltas[1].usage, {"completion_tokens": 2})

    def test_controller_streams_structured_output_and_generation_cancel(self) -> None:
        events = []
        controller = WorkerController(
            launch_nonce="nonce",
            worker_instance_id="worker-qwen3-test",
            model_pack=ModelPack.load(MANIFEST),
            runtime_bundle=RuntimeBundle.load(RUNTIME_BUNDLE),
            emit=events.append,
            transport_factory=lambda config: FakeTransport(config),
        )
        controller.handle(request(1, "handshake", {"launch_nonce": "nonce"}))
        controller.transport = FakeTransport(structured=True)
        controller.lifecycle = "loaded"
        controller.loaded_model_id = "qwen3-4b-instruct-2507-q4-k-m"
        controller.loaded_lease_id = "lease-001"
        schema = {"type": "object"}
        controller.handle(
            request(
                2,
                "infer",
                {
                    "prompt": "Ready?",
                    "max_tokens": 32,
                    "response_json_schema": schema,
                },
            )
        )
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline and not any(event["event"] == "completed" and event["request_id"] == "request-2" for event in events):
            time.sleep(0.01)
        result = next(event for event in events if event["event"] == "llm_result")
        self.assertEqual(result["payload"]["structured_response"]["spoken_response"]["text"], "Ready.")
        controller.transport = FakeTransport(hold=True)
        controller.handle(request(3, "infer", {"prompt": "Long"}))
        time.sleep(0.03)
        controller.handle(request(4, "cancel", generation=1))
        self.assertEqual(controller.generation, 1)
        cancel = next(event for event in events if event["request_id"] == "request-4")
        self.assertTrue(cancel["payload"]["runtime_preserved"])

    def test_framing_round_trip_and_oversize_rejection(self) -> None:
        value = request(1, "health")
        encoded = json.dumps(value).encode()
        input_stream = io.BytesIO(struct.pack(">I", len(encoded)) + encoded)
        output_stream = io.BytesIO()
        frames = FrameIO(input_stream, output_stream)
        self.assertEqual(frames.read(), value)
        frames.write({"ok": True})
        self.assertGreater(len(output_stream.getvalue()), 4)
        with self.assertRaises(LocalLlmError):
            FrameIO(io.BytesIO(struct.pack(">I", 1_048_577)), io.BytesIO()).read()


if __name__ == "__main__":
    unittest.main()
