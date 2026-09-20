from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.request
from typing import Any

DEFAULT_RUNTIME_URL = "http://127.0.0.1:3477"


class TrigoraRuntimeError(RuntimeError):
    def __init__(self, message: str, *, status: int = 0, code: str | None = None) -> None:
        super().__init__(message)
        self.status = status
        self.code = code


def _runtime_url(url: str | None = None) -> str:
    return (url or os.environ.get("TRIGORA_RUNTIME_URL") or DEFAULT_RUNTIME_URL).rstrip("/")


def _request(url: str, *, method: str = "GET", payload: dict[str, Any] | None = None) -> Any:
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={"Accept": "application/json", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request) as response:
            body = response.read().decode("utf-8")
            return json.loads(body) if body else None
    except urllib.error.HTTPError as error:
        raw = error.read().decode("utf-8")
        try:
            parsed = json.loads(raw)
            message = parsed.get("error", {}).get("message") or raw
            code = parsed.get("error", {}).get("code")
        except json.JSONDecodeError:
            message = raw or str(error)
            code = None
        raise TrigoraRuntimeError(message, status=error.code, code=code) from error
    except urllib.error.URLError as error:
        raise TrigoraRuntimeError(
            f"Could not reach the local Trigora runtime at {url}. Is `trigora dev` running? {error.reason}",
            status=0,
        ) from error


class ExecutionHandle:
    def __init__(self, url: str, execution_id: str) -> None:
        self._url = url
        self.id = execution_id

    def send(self, name: str, payload: Any = None) -> None:
        _request(
            f"{self._url}/v1/executions/{self.id}/events",
            method="POST",
            payload={"name": name, "payload": payload if payload is not None else {}},
        )

    def cancel(self) -> None:
        _request(f"{self._url}/v1/executions/{self.id}/cancel", method="POST", payload={})

    def result(self, *, poll_seconds: float = 0.05) -> Any:
        while True:
            body = _request(f"{self._url}/v1/executions/{self.id}")
            execution = body["execution"]
            status = execution["status"]
            if status == "completed":
                return execution.get("result")
            if status == "failed":
                error = execution.get("error") or {}
                raise TrigoraRuntimeError(error.get("message") or "Execution failed.")
            if status == "cancelled":
                raise TrigoraRuntimeError(f'Execution "{self.id}" was cancelled.', status=409)
            time.sleep(poll_seconds)


def start(program_id: str, input: Any | None = None, *, url: str | None = None) -> ExecutionHandle:
    runtime = _runtime_url(url)
    body = _request(
        f"{runtime}/v1/executions",
        method="POST",
        payload={"programId": program_id, "input": input if input is not None else {}},
    )
    return ExecutionHandle(runtime, body["execution"]["id"])
