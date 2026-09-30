# Phase Studio — src/lib tracking fix

## Goal

Restore the Studio package build in CI by making the source helpers imported by
`@valori/studio` visible to git and therefore present in GitHub Actions.

## Delivered

| File | What landed |
|---|---|
| `.gitignore` | Added a narrow unignore for `ui/studio/src/lib/` so Studio source helpers are not hidden by the repository-wide Python `lib/` ignore. |
| `ui/studio/src/lib/**` | Added the Studio helper and hook source tree used by components and `src/index.ts`, including the missing `useGraph`, `useCollectionIndex`, `useHealth`, `useCluster`, `useProof`, `useCollections`, `useEmbeddingConfig`, and `useLLMConfig` modules. |
| `docs/phases/phase-studio-src-lib-tracking.md` | This report. |
| `docs/phases/README.md` | Status-table row for the CI build repair. |
| `CHANGELOG.md` | `[Unreleased]` note for the Studio build fix. |

No root `README.md` or Studio README update was required: no public API,
endpoint, environment variable, or documented workflow changed.

## Findings

- The CI errors were accurate for a clean checkout: `ui/studio/src/lib/` was
  ignored by the root `.gitignore` rule `lib/`.
- The missing `useGraph` types caused the follow-on `GraphView.tsx` implicit
  `any` errors. With the hook source present, `GraphView` type-checks without
  source edits.
- The exact CI-style workspace command is rooted at `ui/`, whose `package.json`
  declares `studio` as an npm workspace.

## Validation

- `npm run build --workspace=studio` from `ui/` — passed (`tsc -p tsconfig.json &&
  tsc-alias -p tsconfig.json --resolve-full-paths`).
- `npm run build --prefix ui/studio` — passed with the same package build script.

## Follow-ups

- Re-run the GitHub Actions build after committing this source tree so the
  CI workspace contains `ui/studio/src/lib/**`.
