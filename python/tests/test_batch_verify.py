#!/usr/bin/env python3
"""Verify batch insert endpoint after Event Log configuration."""

import os
import pytest
import requests

pytestmark = pytest.mark.integration

_BASE_URL = os.getenv("VALORI_URL", "").rstrip("/")


def _base_url() -> str:
    if not _BASE_URL:
        pytest.skip("VALORI_URL not set — export VALORI_URL=https://your-node or set it in .env")
    return _BASE_URL


def test_batch_insert():
    base = _base_url()
    batch = [
        [0.1, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.2, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.3, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.4, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.5, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ]
    resp = requests.post(f"{base}/v1/vectors/batch_insert", json={"batch": batch}, timeout=10)
    assert resp.status_code == 200, f"batch_insert failed ({resp.status_code}): {resp.text}"
    data = resp.json()
    assert "ids" in data, f"unexpected response format: {data}"
    assert len(data["ids"]) == len(batch)
