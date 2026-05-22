"""pagebridge: cognitive retrieval for the database you already have.

Thin Python facade over the PyO3 extension `_pagebridge`. All methods are
async; results that wrap structured data (Answer, DocumentHandle, etc.) are
returned as Python dicts via JSON round-trip.
"""

from __future__ import annotations

import json
from typing import Optional

from ._pagebridge import Pagebridge as _RawPagebridge
from ._pagebridge import __version__  # noqa: F401


class Pagebridge:
    """Async appliance API. Use :func:`open_sqlite` or :func:`open_embedded`."""

    def __init__(self, inner: _RawPagebridge) -> None:
        self._inner = inner

    @classmethod
    async def open_sqlite(
        cls,
        path: str,
        *,
        ollama_url: str = "http://localhost:11434",
        model: str = "qwen2.5:7b",
    ) -> "Pagebridge":
        inner = await _RawPagebridge.open_sqlite(path, ollama_url, model)
        return cls(inner)

    @classmethod
    async def open_embedded(
        cls,
        path: str,
        *,
        ollama_url: str = "http://localhost:11434",
        model: str = "qwen2.5:7b",
    ) -> "Pagebridge":
        inner = await _RawPagebridge.open_embedded(path, ollama_url, model)
        return cls(inner)

    async def ingest_document(
        self,
        text: bytes | str,
        *,
        title: str,
        kind: str = "markdown",
    ) -> dict:
        payload = text if isinstance(text, (bytes, bytearray)) else text.encode("utf-8")
        raw = await self._inner.ingest_document(payload, title, kind)
        return json.loads(raw)

    async def wait_for_summaries(self, doc_id: str) -> None:
        await self._inner.wait_for_summaries(doc_id)

    async def ask(self, question: str) -> dict:
        raw = await self._inner.ask(question)
        return json.loads(raw)

    async def list_documents(self) -> list:
        raw = await self._inner.list_documents()
        return json.loads(raw)

    async def remove_document(self, doc_id: str) -> None:
        await self._inner.remove_document(doc_id)


__all__ = ["Pagebridge"]
