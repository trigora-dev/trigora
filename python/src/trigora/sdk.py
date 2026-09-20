from __future__ import annotations

from collections.abc import Callable
from typing import TypeVar

T = TypeVar("T")


def _require_cli() -> None:
    raise RuntimeError(
        "Durable primitives can only run inside a Trigora execution. Start the program with the Trigora client while `trigora dev` is running."
    )


def effect(name: str, fn: Callable[[], T]) -> T:
    if not isinstance(name, str) or not name.strip():
        raise ValueError("effect(name, fn) requires a non-empty string key.")
    if not callable(fn):
        raise TypeError("effect(name, fn) requires a function.")
    _require_cli()


def wait_for_event(name: str) -> object:
    if not isinstance(name, str) or not name.strip():
        raise ValueError("wait_for_event(name) requires a non-empty string.")
    _require_cli()


def sleep(duration: int | float | str) -> None:
    _require_cli()


def invoke(name: str) -> object:
    if not isinstance(name, str) or not name.strip():
        raise ValueError("invoke(name) requires a non-empty program name.")
    _require_cli()
