# SDK release readiness — npm and PyPI

Phase API-4D §13/§14. **Nothing has been published.** This records what was
verified locally and the exact configuration a future Phase API-4E must put in
place before a first publish.

Two distributions are in scope. Neither is the pre-existing `valoricore`
package (the embedded PyO3 SDK built from `python/`), which has its own release
cadence and its own workflow (`.github/workflows/publish-pypi.yml`). Do not
conflate them.

| Distribution | Registry | Source | Status |
|---|---|---|---|
| `@valori/sdk` | npm | `sdk/typescript` | validated, **not published** |
| `valori` | PyPI | `sdk/python` | validated, **not published** |

---

## npm — `@valori/sdk`

### Verified locally

| Check | Result |
|---|---|
| `npm run build` (tsup, ESM + CJS + d.ts) | pass |
| `npx tsc --noEmit` | pass, 0 errors |
| `npm test` | 223 passed, 3 skipped |
| `npm pack --dry-run` | 10 files, 236.4 kB packed / 1.0 MB unpacked |
| Install packed tarball into a clean project | pass |
| `import` from ESM (`.mjs`) | pass |
| `require` from CJS (`.cjs`) | pass |
| Consumer-side types (`--strict`, `moduleResolution: bundler`) | pass |
| Consumer-side enum safety (`metric: "cosine"` rejected) | pass |
| License files present in tarball | `LICENSE-MIT`, `LICENSE-APACHE` |
| README present in tarball | yes (7.7 kB) |

Package contents are exactly `dist/`, the two licenses, the README and
`package.json` — no tests, no sources, no generated intermediates.

### Configuration added in this phase

`sdk/typescript/package.json` gained:

```json
"publishConfig": {
  "access": "public",
  "provenance": true
}
```

* `access: "public"` is **required**. `@valori/sdk` is a scoped package, and
  npm defaults scoped packages to `restricted`; a first publish without this
  fails (or silently publishes a private package on a paid org).
* `provenance: true` makes npm attach a signed provenance attestation built
  from the GitHub Actions OIDC token. It requires `id-token: write` in the
  publishing job and only works from a supported CI environment.

### Trusted publishing / OIDC — what API-4E must configure

npm trusted publishing removes the long-lived `NPM_TOKEN` entirely.

**On npmjs.com** — package settings → *Trusted Publisher* → GitHub Actions:

| Field | Value |
|---|---|
| Organization / user | `varshith-Git` |
| Repository | `Valori` |
| Workflow filename | `sdk-typescript-release.yml` (to be created) |
| Environment name | `npm-release` |

**In the repository** — a GitHub Environment named `npm-release`, ideally with
required reviewers so a publish is a deliberate act.

**In the workflow** — the publishing job needs exactly:

```yaml
permissions:
  contents: read
  id-token: write        # mints the OIDC token npm exchanges for a publish credential
environment: npm-release
steps:
  - uses: actions/setup-node@v4
    with:
      node-version: "20"
      registry-url: "https://registry.npmjs.org"
  - run: npm ci
    working-directory: sdk/typescript
  - run: npm publish
    working-directory: sdk/typescript
```

`npm` must be **11.5.1 or newer** for trusted publishing; pin it with
`npm install -g npm@latest` in the job if the runner ships an older one.

**Do not add an `NPM_TOKEN` secret.** OIDC is available for this registry and
package, so a stored token is both unnecessary and a standing credential to
leak.

### Open items before a first publish

* The npm name `@valori/sdk` requires the `valori` **org or scope** to exist
  and to be owned by the publisher. This was not verified — it needs a logged-in
  `npm org ls valori` / `npm access` check.
* `version` is `0.1.0` on both SDKs. Decide whether the first public release is
  `0.1.0` or a `0.1.0-rc.N` pre-release, and whether the two SDKs version in
  lockstep with each other and with the API contract version (`1.0`).
* No release workflow exists yet — `sdk-typescript.yml` builds and packs but
  never publishes. Creating it is API-4E's job.

---

## PyPI — `valori`

### Verified locally

| Check | Result |
|---|---|
| `python -m build sdk/python` | sdist + wheel built |
| `twine check dist/*` | `PASSED` for both artifacts |
| Wheel installs into a clean venv | pass |
| `import valori` / `import valori_generated` | pass |
| `valori._wire` present in the wheel | yes |
| `py.typed` present for both packages | `valori/py.typed`, `valori_generated/py.typed` |
| License files in `dist-info` | `LICENSE-MIT`, `LICENSE-APACHE` |
| Unit tests | 307 passed |
| Integration tests vs a real node | 18 passed, 3 skipped |

Artifacts: `valori-0.1.0-py3-none-any.whl` (296 files) and
`valori-0.1.0.tar.gz`.

### Metadata

* `requires-python = ">=3.9"`, with classifiers for 3.9–3.13.
* `license = "MIT OR Apache-2.0"` with `license-files` declared — PEP 639 form,
  which is why the licenses land in `dist-info/licenses/`.
* Runtime dependencies are pinned to ranges matching
  `sdk/generator.lock.json`: `httpx>=0.23.0,<0.29.0`, `attrs>=22.2.0`,
  `python-dateutil>=2.8.0`.
* README is the long description.

**Note:** the wheel is `py3-none-any` and the declared floor is 3.9, but the
test runs above used 3.13 (unit/integration) and a 3.9 interpreter only for
`build`/`twine`. A `python-version` matrix covering 3.9 through 3.13 should gate
the first publish; the current `sdk-python.yml` test job pins a single version.

### Trusted publishing / OIDC — what API-4E must configure

**On pypi.org** — the *project* `valori` → *Publishing* → add a GitHub
publisher:

| Field | Value |
|---|---|
| Owner | `varshith-Git` |
| Repository | `Valori` |
| Workflow name | `sdk-python-release.yml` (to be created) |
| Environment name | `pypi-release` |

Because `valori` does not exist on PyPI yet, this must be registered as a
**pending publisher** (PyPI → *Your projects* → *Publishing* → *Add a pending
publisher*), which creates the project on first successful upload.

**In the repository** — a GitHub Environment named `pypi-release`, with
required reviewers.

**In the workflow** — the publishing job needs exactly:

```yaml
permissions:
  contents: read
  id-token: write        # required for PyPI Trusted Publishers (OIDC)
environment: pypi-release
steps:
  - run: python -m build sdk/python
  - run: twine check sdk/python/dist/*
  - uses: pypa/gh-action-pypi-publish@release/v1
    with:
      packages-dir: sdk/python/dist
```

No `password:` / `PYPI_API_TOKEN` — the action detects the OIDC token.

`.github/workflows/publish-pypi.yml` already uses `id-token: write` with
`pypa/gh-action-pypi-publish`, but it publishes **`valoricore` from
`python/`**, not this distribution. It is a working reference for the shape of
the job; it must not be repointed at `sdk/python`.

### Open items before a first publish

* The name `valori` on PyPI was not verified as available. Check before
  registering the pending publisher.
* `docs/publishing-pypi.md` documents a manual, API-token-based flow for
  `valoricore`. It predates trusted publishing and should not be used as the
  template for this distribution.
* Decide the version/tag trigger (`sdk-python-v*` vs a shared release tag) and
  whether TestPyPI gets a dry run first.

---

## Summary

Both packages **build, install, import and typecheck** from their published
artifacts, and both pass their full test suites including real-node
integration. Neither has been published, no registry credential has been
created or stored, and no publish workflow exists yet — that is Phase API-4E.
