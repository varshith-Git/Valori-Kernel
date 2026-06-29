"""
pytest configuration for valoricore tests.

Test categories
---------------
Unit tests (no node, no network — run anywhere):
    pytest -m unit

Integration tests (require a running Valoricore node on localhost:3000):
    VALORI_URL=http://localhost:3000 pytest -m integration

All offline tests (default — skips anything needing a node):
    pytest                          # same as: pytest -m "not integration"

Run everything:
    pytest -m ""

Install dev dependencies first:
    pip install "valoricore[dev]"    # includes pytest-asyncio
"""

import os
import pytest

# Ensure no leftover VALORI_* env vars from prior runs pollute FFI engine init
for _k in list(os.environ):
    if _k.startswith("VALORI_"):
        del os.environ[_k]


# ── Markers ───────────────────────────────────────────────────────────────────

def pytest_configure(config):
    config.addinivalue_line(
        "markers",
        "unit: pure offline test — no node, no network, no FFI required",
    )
    config.addinivalue_line(
        "markers",
        "integration: requires a running Valoricore node (set VALORI_URL or use localhost:3000)",
    )
    config.addinivalue_line(
        "markers",
        "ffi: requires the compiled Rust FFI extension (maturin develop)",
    )


# ── Auto-skip integration tests when no node is reachable ────────────────────

def _node_url() -> str:
    return os.environ.get("VALORI_URL", "http://localhost:3000").rstrip("/")


def _node_is_up() -> bool:
    try:
        import requests
        r = requests.get(f"{_node_url()}/health", timeout=2)
        return r.status_code == 200
    except Exception:
        return False


_NODE_UP = None  # Optional[bool], cached once per session


def _node_available() -> bool:
    global _NODE_UP
    if _NODE_UP is None:
        _NODE_UP = _node_is_up()
    return _NODE_UP


def pytest_collection_modifyitems(config, items):
    node_up = _node_available()

    skip_integration = pytest.mark.skip(
        reason=f"Valoricore node not reachable at {_node_url()} — "
               "start a node or set VALORI_URL to run integration tests"
    )

    # Also detect FFI availability once
    try:
        import valoricore.valoricore_ffi  # noqa: F401
        ffi_available = True
    except ImportError:
        ffi_available = False

    skip_ffi = pytest.mark.skip(
        reason="Rust FFI extension not built — run 'maturin develop' inside python/"
    )

    for item in items:
        if "integration" in item.keywords and not node_up:
            item.add_marker(skip_integration)
        if "ffi" in item.keywords and not ffi_available:
            item.add_marker(skip_ffi)


# ── Shared fixtures ───────────────────────────────────────────────────────────

@pytest.fixture(scope="session")
def node_url() -> str:
    """Base URL of the test node. Skips the test if node is not reachable."""
    if not _node_available():
        pytest.skip(f"Valoricore node not reachable at {_node_url()}")
    return _node_url()


@pytest.fixture
def client(node_url):
    """A SyncRemoteClient pointed at the test node."""
    from valoricore.remote import SyncRemoteClient
    return SyncRemoteClient(node_url)


@pytest.fixture
def async_client(node_url):
    """An AsyncRemoteClient pointed at the test node."""
    from valoricore.remote import AsyncRemoteClient
    return AsyncRemoteClient(node_url)
