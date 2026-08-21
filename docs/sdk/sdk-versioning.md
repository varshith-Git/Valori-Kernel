# Valori SDK versioning

**Status:** Phase API-4A §2/§14.

Three version numbers are in play. Conflating them is the failure mode this
document exists to prevent.

| Version | Example | Owned by | Moves when |
|---|---|---|---|
| **API contract version** | `1.0` | `api/openapi/valori-v1.yaml` → `info.version` | The REST contract changes shape. |
| **SDK package version** | `0.1.0` | `pyproject.toml` / `package.json` | The SDK changes, including fixes that touch no endpoint. |
| **Generator version** | `0.26.2`, `13.12.6` | `sdk/generator.lock.json` | Someone deliberately bumps a generator. |

## 1. API contract version

The contract declares `info.version: 1.0.0`. SDKs record its **major.minor**,
`1.0`, as `API_CONTRACT_VERSION`. Every SDK exposes it:

```python
from valori import API_CONTRACT_VERSION, ValoriClient
ValoriClient("http://localhost:3000").api_contract_version   # "1.0"
```

```ts
import { API_CONTRACT_VERSION, ValoriClient } from "@valori/sdk";
new ValoriClient({ endpoint: "…" }).apiContractVersion;      // "1.0"
```

This is not a comment. Four places must agree, and CI fails if they drift:

* `api/openapi/valori-v1.yaml` → `info.version`
* `sdk/generator.lock.json` → `contract.info_version`
* `sdk/python/pyproject.toml` → `[tool.valori] api_contract_version`
* `sdk/typescript/package.json` → `valori.apiContractVersion`

Enforced by `sdk/python/tests/test_contract_version.py` and
`sdk/typescript/tests/version.test.ts`.

### Compatibility is checked, not assumed

```python
from valori import check_api_compatibility
check_api_compatibility(node_reported_version)   # raises on a different major
```

A node reporting an incompatible major is an error, not something to shrug at.
An **unparseable** version is also an error — the SDK never assumes
compatibility it cannot verify.

Supported range today: `1.0` – `1.x`. A `2.x` node needs a `2.x` SDK.

## 2. SDK package version

Independent of the contract, on purpose. A bug in the retry backoff is a patch
release of the SDK and touches no endpoint; it must not imply an API change.

Both SDKs start at `0.1.0`. Semantics once published:

| Change | Bump |
|---|---|
| Bug fix in the handwritten layer | patch |
| New ergonomic wrapper over an existing operation | minor |
| Regeneration after a backwards-compatible contract addition | minor |
| Breaking change to the SDK's own surface | major |
| Regeneration after a breaking contract change | major, and `API_CONTRACT_VERSION` moves too |

A test asserts the two versions are not the same string, so nobody can
accidentally couple them.

## 3. Generator version

`sdk/generator.lock.json` pins every tool in the pipeline to an exact version:

```json
"python":     { "generator": "openapi-python-client", "version": "0.26.2",
                "formatter": "ruff", "formatter_version": "0.13.3" },
"typescript": { "generator": "swagger-typescript-api", "version": "13.12.6" }
```

No `latest`, anywhere. A floating generator makes "generated == regenerated"
unprovable, which is the whole point of the reproducibility gate. A test asserts
each pin starts with a digit.

Bumping a generator is a deliberate, reviewed act and must arrive as one commit
containing: the lockfile change, the regenerated `generated/` tree, any
handwritten adjustments the new output requires, and a CHANGELOG entry.

## 4. What a contract change does to the SDKs

```
contract change
      ↓
api-contract.yml           gate must stay green
      ↓
scripts/generate.sh        regenerate, commit the diff
      ↓
sdk-repro-check.sh         committed == regenerated, twice over
      ↓
sdk-coverage-check.py      74/74 still, or the build fails
      ↓
handwritten wrapper        a human writes it; nothing auto-generates semantics
      ↓
tests                      unit + integration
      ↓
version bump               per the table above
```

An added operation fails the coverage gate until someone writes the wrapper.
That is intended: the alternative is a released SDK that silently cannot reach
part of the API.

## 5. Release tags

| Tag | Publishes |
|---|---|
| `sdk-python-v<version>` | `valori` to PyPI |
| `sdk-ts-v<version>` | `@valori/sdk` to npm |

Neither is enabled in API-4A — see [`sdk-release-process.md`](sdk-release-process.md).
