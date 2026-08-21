# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Generated-client contract tests — Phase API-4A §17.

These assert on the *generated* layer, not the wrappers: every public operation
in ``api/openapi/valori-v1.yaml`` must have a callable generated endpoint module
whose URL and method match the contract. If the generated tree ever falls behind
the committed contract, this fails before any wrapper does.
"""

from __future__ import annotations

import functools
import importlib
import pathlib
import pkgutil
import re

import pytest

import valori_generated
from valori_generated import api as generated_api

from .conftest import CONTRACT_PATH

METHODS = {"get", "post", "put", "delete", "patch", "head", "options", "trace"}


def contract_operations(contract: dict) -> dict:
    """``operationId -> (method, path)`` for every operation in the contract."""
    out = {}
    for path, item in (contract.get("paths") or {}).items():
        for method, op in (item or {}).items():
            if method.lower() in METHODS:
                out[op["operationId"]] = (method.lower(), path)
    return out


@functools.lru_cache(maxsize=1)
def generated_modules() -> dict:
    """``module_name -> module`` for every generated endpoint module."""
    out = {}
    for tag in pkgutil.iter_modules(generated_api.__path__):
        if not tag.ispkg:
            continue
        pkg = importlib.import_module(f"valori_generated.api.{tag.name}")
        for mod in pkgutil.iter_modules(pkg.__path__):
            out[mod.name] = importlib.import_module(
                f"valori_generated.api.{tag.name}.{mod.name}")
    return out


def test_the_generated_tree_covers_every_contract_operation(contract):
    expected = set(contract_operations(contract))
    produced = set(generated_modules())
    assert expected - produced == set(), f"operations with no generated module: {expected - produced}"
    assert produced - expected == set(), f"generated modules with no contract operation: {produced - expected}"


def test_the_contract_still_has_74_public_operations(contract):
    """The API-3.3 baseline this phase was built on.

    Not a magic number for its own sake: the SDK, the coverage manifests and the
    docs all state 74, and a contract that quietly grows or shrinks should make
    those statements fail rather than become stale.
    """
    assert len(contract_operations(contract)) == 74


def test_every_generated_module_exposes_the_four_call_forms():
    for name, module in generated_modules().items():
        for fn in ("sync", "sync_detailed", "asyncio", "asyncio_detailed"):
            assert callable(getattr(module, fn, None)), f"{name} is missing {fn}()"


def _operation_ids_for_parametrize() -> list:
    """Operation ids read at collection time, so each gets its own test case.

    Returns an empty list when the contract is not on disk (a stripped sdist),
    which parametrizes to zero cases rather than erroring during collection.
    """
    try:
        import yaml
    except ImportError:  # pragma: no cover
        return []
    if not CONTRACT_PATH.exists():  # pragma: no cover
        return []
    return sorted(contract_operations(yaml.safe_load(CONTRACT_PATH.read_text())))


@pytest.mark.parametrize("operation_id", _operation_ids_for_parametrize())
def test_generated_module_targets_the_contract_url_and_method(operation_id, contract):
    """The generated `_get_kwargs` must name the same method and path as the contract."""
    module = generated_modules()[operation_id]
    method, path = contract_operations(contract)[operation_id]

    source = open(module.__file__, encoding="utf-8").read()
    declared_method = re.search(r'"method":\s*"([a-z]+)"', source)
    declared_url = re.search(r'"url":\s*f?"([^"]+)"', source)
    assert declared_method and declared_url, f"{operation_id}: no method/url in _get_kwargs"

    assert declared_method.group(1) == method

    # The generated URL is an f-string with `{param}` holes, same as the
    # contract's templated path — modulo the generator's `id=...` formatting.
    normalised = re.sub(r"\{[^}]*\}", "{}", declared_url.group(1))
    assert normalised == re.sub(r"\{[^}]*\}", "{}", path)


def test_the_generated_package_does_not_import_the_handwritten_one():
    """§4: the arrow points one way. Generated must never reach up into `valori`."""
    root = pathlib.Path(valori_generated.__file__).parent
    offenders = []
    for py in root.rglob("*.py"):
        text = py.read_text(encoding="utf-8")
        if re.search(r"^\s*(from|import)\s+valori(\.|\s|$)", text, re.M):
            offenders.append(str(py.relative_to(root)))
    assert offenders == [], f"generated code imports the handwritten layer: {offenders}"
