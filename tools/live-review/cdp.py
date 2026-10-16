"""Evaluate JavaScript in the review app's WebView2 over the DevTools protocol.

Start the app with remote debugging first, e.g. in PowerShell:
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333'
  Start-Process 'E:\\work bench\\Interactive LLM\\review\\interactive-npcs-control.exe'

Usage:
  python -I cdp.py "document.title"
  python -I cdp.py --file snippet.js
  python -I cdp.py --text            # visible page text

The expression may return a promise; it is awaited. Results print as JSON.
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request

import websocket


def page_socket(port: int) -> str:
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/json/list", timeout=5) as response:
        targets = json.load(response)
    pages = [target for target in targets if target.get("type") == "page"]
    if not pages:
        raise SystemExit(f"no page targets on port {port}")
    return pages[0]["webSocketDebuggerUrl"]


def evaluate(port: int, expression: str, timeout: float) -> object:
    socket = websocket.create_connection(page_socket(port), timeout=timeout, suppress_origin=True)
    try:
        socket.send(json.dumps({
            "id": 1,
            "method": "Runtime.evaluate",
            "params": {"expression": expression, "awaitPromise": True, "returnByValue": True},
        }))
        while True:
            message = json.loads(socket.recv())
            if message.get("id") == 1:
                break
    finally:
        socket.close()
    result = message.get("result", {})
    if "exceptionDetails" in result:
        raise SystemExit("exception: " + json.dumps(result["exceptionDetails"], indent=1)[:4000])
    return result.get("result", {}).get("value")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("expression", nargs="?")
    parser.add_argument("--file")
    parser.add_argument("--text", action="store_true")
    parser.add_argument("--port", type=int, default=9333)
    parser.add_argument("--timeout", type=float, default=30.0)
    args = parser.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")
    if args.text:
        expression = "document.body.innerText"
    elif args.file:
        with open(args.file, encoding="utf-8") as handle:
            expression = handle.read()
    elif args.expression:
        expression = args.expression
    else:
        raise SystemExit("give an expression, --file or --text")
    value = evaluate(args.port, expression, args.timeout)
    print(value if isinstance(value, str) else json.dumps(value, indent=1))


if __name__ == "__main__":
    main()
