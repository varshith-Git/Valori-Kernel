# Valori SDK release process

**Status:** Phase API-4A §19/§20 — the pipeline is wired and proven up to the
publish step. **Nothing has been published to PyPI or npm.**

---

## 1. Where the pipeline currently stops

```
Valori API release
      ↓
OpenAPI contract              api-contract.yml            ✅ wired
      ↓
SDK generation                sdk/*/scripts/generate.sh   ✅ wired
      ↓
reproducibility               sdk-repro-check.sh          ✅ wired · passing
      ↓
handwritten coverage check    sdk-coverage-check.py       ✅ wired · 74/74
      ↓
tests                         unit + integration          ✅ wired · passing
      ↓
package build                 build/twine · tsup/npm pack ✅ wired · passing
      ↓
release approval              GitHub environment          ⛔ environment not created
      ↓
PyPI / npm                                                ⛔ NOT DONE
```

The `publish` jobs exist in `sdk-python.yml` and `sdk-typescript.yml`, but each
declares a GitHub **environment** (`pypi`, `npm`) that does not exist in the
repository. A release tag therefore runs everything and then stops, waiting for
an environment somebody has to create on purpose. That is the intended state at
the end of API-4A.

## 2. Trusted publishing, not stored passwords

Both publish jobs use OIDC:

* **PyPI** — `pypa/gh-action-pypi-publish` with `permissions: id-token: write`.
  PyPI trusted publishing must be configured for the project, naming this
  repository, the workflow file, and the `pypi` environment. No API token is
  ever stored in repository secrets.
* **npm** — `npm publish --provenance --access public` with
  `permissions: id-token: write`, which attaches a signed provenance statement
  linking the tarball to this workflow run.

If OIDC is genuinely unavailable for a registry, that is a decision to document
and scope, not to work around with a long-lived token committed to secrets.

## 3. Enabling publishing (a later phase)

1. Create the `pypi` and `npm` environments in repository settings, each with
   required reviewers. The reviewer gate *is* the "release approval" step in
   the flow above.
2. Configure PyPI trusted publishing for `valori`, and npm trusted publishing /
   provenance for `@valori/client-sdk`.
3. Reserve both names. `valori` on PyPI and `@valori/client-sdk` on npm have **not**
   been checked for availability as part of API-4A.
4. Do a dry run against TestPyPI and `npm publish --dry-run` before the first
   real release.

## 4. Cutting a release, once enabled

```bash
# 1. The contract must be green first. Everything downstream assumes it.
./scripts/api-contract-gate.sh

# 2. Regenerate and prove reproducibility.
./sdk/python/scripts/generate.sh
./sdk/typescript/scripts/generate.sh
./scripts/sdk-repro-check.sh

# 3. Coverage must be complete and every claim must resolve.
python3 scripts/sdk-coverage-check.py

# 4. Test.
python -m pytest sdk/python/tests -q -m "not integration"
npm --prefix sdk/typescript test

# 5. Integration, against a real node.
cargo run -p valori-node &
VALORI_TEST_ENDPOINT=http://localhost:3000 \
  python -m pytest sdk/python/tests -q -m integration
VALORI_TEST_ENDPOINT=http://localhost:3000 npm --prefix sdk/typescript test

# 6. Bump versions (see sdk-versioning.md) and update CHANGELOG.md.

# 7. Build locally and eyeball the artifacts.
python -m build sdk/python && twine check sdk/python/dist/*
npm --prefix sdk/typescript run build && npm --prefix sdk/typescript pack --dry-run

# 8. Tag. The tag is what triggers the publish job.
git tag sdk-python-v0.1.0 && git push origin sdk-python-v0.1.0
git tag sdk-ts-v0.1.0     && git push origin sdk-ts-v0.1.0
```

## 5. What CI refuses to let through

Per §21, a build fails when:

| Condition | Caught by |
|---|---|
| The OpenAPI contract changed unexpectedly | `api-contract.yml` — diffs the committed contract against the generator's output |
| A generated SDK differs from the committed tree | `sdk-repro-check.sh` |
| Generation is not byte-stable across two runs | `sdk-repro-check.sh` |
| SDK coverage is incomplete, or a wrapper claim is false | `sdk-coverage-check.py` |
| SDK tests fail | `sdk-python.yml` / `sdk-typescript.yml` `test` + `integration` jobs |
| A package will not build, or its metadata is unpublishable | `build` jobs (`twine check`, `npm pack --dry-run`) |
| The recorded API-contract version drifts from the contract | `test_contract_version.py` / `version.test.ts` |
| A generator pin becomes `latest` | same tests |

The `build` job also installs the freshly built wheel into a clean virtualenv,
imports it, and asserts the API key does not appear in `repr(client)` — a
release must not be the first place that regression is noticed.

## 6. Rollback

Neither PyPI nor npm allows overwriting a published version. A bad release is
fixed forward with a new patch version; on npm the bad version can additionally
be deprecated (`npm deprecate`). Yanking on PyPI hides a release from resolvers
but does not delete it. Plan releases accordingly: the local build and
`--dry-run` steps in §4 are cheap, and they are the last point at which a
mistake costs nothing.
