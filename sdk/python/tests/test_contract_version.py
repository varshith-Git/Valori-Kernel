# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""API-contract version tests — Phase API-4A §14.

The SDK records which Valori API contract it targets. These tests make that
record load-bearing: it must agree with the OpenAPI document, with
``pyproject.toml``, and with the pinned generator lockfile. Drift in any of the
four is a failing build, not a stale comment.
"""

from __future__ import annotations

import json
import pathlib

import pytest

from valori import API_CONTRACT_VERSION, ValoriClient, __version__
from valori.errors import ValoriConfigError
from valori.version import check_api_compatibility

from .conftest import REPO_ROOT

PYPROJECT = pathlib.Path(__file__).resolve().parents[1] / "pyproject.toml"
LOCKFILE = REPO_ROOT / "sdk" / "generator.lock.json"


def _read_pyproject() -> dict:
    try:
        import tomllib
    except ModuleNotFoundError:  # Python 3.9/3.10
        tomllib = pytest.importorskip("tomli")
    return tomllib.loads(PYPROJECT.read_text())


def test_the_sdk_targets_the_contracts_declared_version(contract):
    """``info.version`` is ``1.0.0``; the SDK targets its major.minor, ``1.0``."""
    info_version = contract["info"]["version"]
    assert info_version.startswith(API_CONTRACT_VERSION + ".")


def test_the_contract_is_openapi_31(contract):
    assert contract["openapi"] == "3.1.0"


def test_pyproject_records_the_same_contract_version():
    data = _read_pyproject()
    assert data["tool"]["valori"]["api_contract_version"] == API_CONTRACT_VERSION
    assert data["tool"]["valori"]["openapi_version"] == "3.1.0"


def test_the_generator_lockfile_agrees(contract):
    if not LOCKFILE.exists():  # pragma: no cover
        pytest.skip("generator lockfile not present in this tree")
    lock = json.loads(LOCKFILE.read_text())
    assert lock["contract"]["api_contract_version"] == API_CONTRACT_VERSION
    assert lock["contract"]["openapi_version"] == contract["openapi"]
    assert lock["contract"]["info_version"] == contract["info"]["version"]


def test_the_python_generator_pin_is_exact():
    """§2: no `latest` anywhere in the SDK pipeline."""
    if not LOCKFILE.exists():  # pragma: no cover
        pytest.skip("generator lockfile not present in this tree")
    lock = json.loads(LOCKFILE.read_text())
    for section in ("python", "typescript"):
        version = lock[section]["version"]
        assert version != "latest"
        assert version[0].isdigit(), f"{section} generator pin is not an exact version"


def test_the_package_version_is_independent_of_the_contract_version():
    data = _read_pyproject()
    assert data["project"]["version"] == __version__
    assert __version__ != API_CONTRACT_VERSION


def test_the_client_exposes_the_contract_it_targets():
    client = ValoriClient("http://node.test")
    assert client.api_contract_version == API_CONTRACT_VERSION
    assert API_CONTRACT_VERSION in repr(client)


def test_a_matching_major_is_accepted():
    check_api_compatibility("1.0")
    check_api_compatibility("1.4.2")


def test_an_incompatible_major_is_refused_loudly():
    with pytest.raises(ValoriConfigError) as caught:
        check_api_compatibility("2.0")
    assert "2.0" in str(caught.value)
    assert "1.0" in str(caught.value)


def test_an_unparseable_version_is_refused_rather_than_assumed_compatible():
    with pytest.raises(ValoriConfigError):
        check_api_compatibility("not-a-version")
