#!/bin/sh
# Installs the REAL valoricore SDK — the same, unmodified source tree at
# Valori-Kernel/python, not a fake/mock client — plus test-only deps, then
# runs whatever command was passed (default: pytest -v).
#
# `pip install -e` is deliberately NOT used here: valoricore's pyproject.toml
# build-backend is maturin (crates/valori-ffi, the embedded FFI extension
# for MemoryClient/LocalClient — a totally different code path from what
# this suite tests). Building it needs the whole Rust workspace + a system
# C toolchain in this container, none of which this remote-HTTP-only test
# suite exercises: `valoricore.local`'s FFI import is a module-level
# try/except ImportError -> _ffi = None (python/valoricore/local.py),
# only raised inside LocalClient.__init__, which nothing here calls.
# PYTHONPATH points straight at the real, unmodified source package
# instead — same code, just skipping an irrelevant native build step.
set -eu
export PYTHONPATH="/valoricore-src:${PYTHONPATH:-}"
pip install -q requests numpy pydantic typing-extensions httpx
pip install -q -r /e2e/requirements.txt
exec "$@"
