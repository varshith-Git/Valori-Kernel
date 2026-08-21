// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Version identity — Phase API-4A §14.
//
// Two versions, deliberately separate:
//
//   VERSION              the SDK package version; patch-level SDK fixes move
//                        this and nothing else.
//   API_CONTRACT_VERSION the Valori REST API contract this SDK was generated
//                        against. `tests/version.test.ts` checks it against
//                        api/openapi/valori-v1.yaml and package.json, so the
//                        two cannot drift silently.

import { ValoriConfigError } from "./errors.js";

export const VERSION = "0.1.0";

/** major.minor of the contract in api/openapi/valori-v1.yaml (info.version 1.0.0). */
export const API_CONTRACT_VERSION = "1.0";

export const MIN_SUPPORTED_API_CONTRACT = "1.0";
export const MAX_SUPPORTED_API_CONTRACT = "1.x";

function major(version: string): number | undefined {
  const head = Number.parseInt(String(version).split(".")[0] ?? "", 10);
  return Number.isNaN(head) ? undefined : head;
}

/**
 * Throw if `nodeApiVersion` is outside this SDK's supported range.
 *
 * Call it with the `api_version` a node reports from `GET /v1/version`.
 * Silence about an incompatible major is exactly the failure mode §14 forbids.
 */
export function checkApiCompatibility(nodeApiVersion: string): void {
  const nodeMajor = major(nodeApiVersion);
  const wantMajor = major(API_CONTRACT_VERSION);
  if (nodeMajor === undefined) {
    throw new ValoriConfigError(
      `node reported an unparseable API version: ${nodeApiVersion}`,
    );
  }
  if (nodeMajor !== wantMajor) {
    throw new ValoriConfigError(
      `this SDK targets Valori API contract ${API_CONTRACT_VERSION} ` +
        `(supported: ${MIN_SUPPORTED_API_CONTRACT}–${MAX_SUPPORTED_API_CONTRACT}), ` +
        `but the node reports ${nodeApiVersion}. Install a matching SDK.`,
    );
  }
}
