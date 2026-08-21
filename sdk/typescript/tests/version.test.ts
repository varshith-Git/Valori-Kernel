// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// API-contract version tests — Phase API-4A §14.
//
// The recorded contract version is load-bearing: it must agree with the OpenAPI
// document, with package.json, and with the pinned generator lockfile. Drift in
// any of the four is a failing build, not a stale comment.

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import {
  API_CONTRACT_VERSION,
  VERSION,
  ValoriClient,
  ValoriConfigError,
  checkApiCompatibility,
} from "../src/index.js";
import { REPO_ROOT, SDK_ROOT, contractInfoVersion } from "./helpers.js";

const pkg = JSON.parse(readFileSync(path.join(SDK_ROOT, "package.json"), "utf8"));
const lock = JSON.parse(readFileSync(path.join(REPO_ROOT, "sdk/generator.lock.json"), "utf8"));

describe("contract version", () => {
  it("targets the contract's declared version", () => {
    expect(contractInfoVersion().startsWith(`${API_CONTRACT_VERSION}.`)).toBe(true);
  });

  it("is recorded in package.json", () => {
    expect(pkg.valori.apiContractVersion).toBe(API_CONTRACT_VERSION);
    expect(pkg.valori.openapiVersion).toBe("3.1.0");
  });

  it("agrees with the generator lockfile", () => {
    expect(lock.contract.api_contract_version).toBe(API_CONTRACT_VERSION);
    expect(lock.contract.info_version).toBe(contractInfoVersion());
  });

  it("pins the generator exactly — no `latest` in the pipeline", () => {
    for (const section of ["python", "typescript"]) {
      expect(lock[section].version).not.toBe("latest");
      expect(/^\d/.test(lock[section].version)).toBe(true);
    }
  });

  it("keeps the package version independent of the contract version", () => {
    expect(pkg.version).toBe(VERSION);
    expect(VERSION).not.toBe(API_CONTRACT_VERSION);
  });

  it("exposes the contract it targets on the client", () => {
    const client = new ValoriClient({ endpoint: "http://node.test" });
    expect(client.apiContractVersion).toBe(API_CONTRACT_VERSION);
    expect(String(client)).toContain(API_CONTRACT_VERSION);
  });
});

describe("compatibility checking", () => {
  it("accepts a matching major", () => {
    expect(() => checkApiCompatibility("1.0")).not.toThrow();
    expect(() => checkApiCompatibility("1.4.2")).not.toThrow();
  });

  it("refuses an incompatible major loudly", () => {
    expect(() => checkApiCompatibility("2.0")).toThrow(ValoriConfigError);
    expect(() => checkApiCompatibility("2.0")).toThrow(/2\.0/);
  });

  it("refuses an unparseable version rather than assuming compatibility", () => {
    expect(() => checkApiCompatibility("not-a-version")).toThrow(ValoriConfigError);
  });
});
