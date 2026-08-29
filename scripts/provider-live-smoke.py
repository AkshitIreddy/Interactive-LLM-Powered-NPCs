#!/usr/bin/env python3
"""Low-cost live credential smoke tests with content-free output.

The input file stays git-ignored. Keys are read into request headers only and
never printed, written to the result, placed in URLs, or passed on a command
line. These checks validate authentication/account access; deterministic mock
tests remain the source of truth for streaming wire behavior.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import ssl
import urllib.error
import urllib.request
from pathlib import Path


PROVIDER_ALIASES = {
    "cohere": ("cohere",),
    "elevenlabs": ("elevenlabs", "eleven labs", "elevenlab"),
    "assemblyai": ("assemblyai", "assembly ai"),
}


def read_credentials(path: Path) -> dict[str, list[tuple[str, str]]]:
    found: dict[str, list[tuple[str, str]]] = {provider: [] for provider in PROVIDER_ALIASES}
    current: str | None = None
    for raw in path.read_text(encoding="utf-8-sig").splitlines():
        line = raw.strip()
        if not line:
            continue
        lowered = line.lower()
        provider = next(
            (
                provider
                for provider, aliases in PROVIDER_ALIASES.items()
                if any(alias in lowered for alias in aliases)
            ),
            None,
        )
        if provider:
            current = provider
            match = re.search(r"[:=]\s*([^\s].*)$", line)
            if match:
                value = match.group(1).strip().strip("\"'")
                if len(value) >= 8:
                    credential_class = (
                        "production"
                        if "prod" in lowered or "production" in lowered
                        else "trial_or_unspecified"
                    )
                    found[provider].append((credential_class, value))
                    current = None
            continue
        if current and len(line) >= 8:
            found[current].append(("trial_or_unspecified", line.strip("\"'")))
            current = None
    return found


def request_json(url: str, headers: dict[str, str], timeout: float) -> tuple[int, dict]:
    request = urllib.request.Request(
        url,
        headers={
            **headers,
            "Accept": "application/json",
            "User-Agent": "Interactive-NPCs-2.0-local-smoke",
        },
        method="GET",
    )
    context = ssl.create_default_context()
    with urllib.request.urlopen(request, timeout=timeout, context=context) as response:
        payload = response.read(2 * 1024 * 1024 + 1)
        if len(payload) > 2 * 1024 * 1024:
            raise ValueError("response_too_large")
        decoded = json.loads(payload)
        if not isinstance(decoded, dict):
            raise ValueError("unexpected_response_shape")
        return response.status, decoded


def checked_call(provider: str, credential_class: str, key: str, timeout: float) -> dict:
    if provider == "cohere":
        url = "https://api.cohere.com/v1/models?endpoint=chat"
        headers = {"Authorization": f"Bearer {key}", "X-Client-Name": "interactive-npcs-local-smoke"}
    elif provider == "elevenlabs":
        url = "https://api.elevenlabs.io/v1/user/subscription"
        headers = {"xi-api-key": key}
    elif provider == "assemblyai":
        url = "https://api.assemblyai.com/v2/transcript?limit=1"
        headers = {"Authorization": key}
    else:
        raise ValueError("unsupported_provider")

    result = {
        "provider": provider,
        "credentialClass": credential_class,
        "authenticated": False,
        "status": "transport_error",
        "httpStatus": None,
        "metadata": {},
    }
    try:
        status, payload = request_json(url, headers, timeout)
        result["httpStatus"] = status
        result["authenticated"] = status == 200
        result["status"] = "ok" if status == 200 else "unexpected_status"
        if provider == "cohere":
            result["metadata"] = {"modelRecordsVisible": len(payload.get("models", []))}
        elif provider == "elevenlabs":
            result["metadata"] = {
                "tier": payload.get("tier", "unknown"),
                "accountStatus": payload.get("status", "unknown"),
                "quotaPresent": isinstance(payload.get("character_limit"), int),
            }
        elif provider == "assemblyai":
            page = payload.get("page_details") or {}
            result["metadata"] = {
                "historyEndpointAccessible": isinstance(payload.get("transcripts"), list),
                "resultCount": page.get("result_count") if isinstance(page, dict) else None,
            }
    except urllib.error.HTTPError as error:
        result["httpStatus"] = error.code
        result["status"] = {
            401: "invalid_credential",
            403: "forbidden_or_scope_limited",
            429: "throttled",
        }.get(error.code, "provider_http_error")
    except (urllib.error.URLError, TimeoutError):
        result["status"] = "network_or_timeout"
    except (ValueError, json.JSONDecodeError):
        result["status"] = "invalid_response"
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--credentials",
        type=Path,
        default=Path(".secrets/Commonly used Keys.txt"),
    )
    parser.add_argument("--out", type=Path)
    parser.add_argument("--timeout", type=float, default=15.0)
    args = parser.parse_args()
    credentials = read_credentials(args.credentials)
    results = []

    cohere = sorted(credentials["cohere"], key=lambda item: item[0] == "production")
    if cohere:
        first = checked_call("cohere", cohere[0][0], cohere[0][1], args.timeout)
        results.append(first)
        if first["status"] == "throttled":
            production = next((item for item in cohere if item[0] == "production"), None)
            if production:
                results.append(checked_call("cohere", *production, timeout=args.timeout))

    for provider in ("elevenlabs", "assemblyai"):
        if credentials[provider]:
            credential_class, key = credentials[provider][0]
            results.append(checked_call(provider, credential_class, key, args.timeout))

    report = {
        "schemaVersion": 1,
        "checkedAtUtc": dt.datetime.now(dt.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "purpose": "local-authentication-smoke-only",
        "containsCredentialValues": False,
        "results": results,
    }
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    if not results or any(not item["authenticated"] for item in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
