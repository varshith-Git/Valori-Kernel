// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// Generated-client contract tests — Phase API-4A §17.
//
// These assert on the *generated* layer, not the wrappers: every public
// operation in api/openapi/valori-v1.yaml must have a callable generated method
// on `GeneratedApi`. If the generated file falls behind the committed contract,
// this fails before any wrapper does.

import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { GeneratedApi, HttpClient } from "../generated/valori-api.js";
import { SDK_ROOT, contractOperationIds, makeClient, noContent } from "./helpers.js";

const GENERATED = path.join(SDK_ROOT, "generated/valori-api.ts");
const source = readFileSync(GENERATED, "utf8");

/** camelCase form of a snake_case operationId — the generator's naming rule. */
function camel(operationId: string): string {
  const [head, ...rest] = operationId.split("_");
  return head + rest.map((w) => w[0].toUpperCase() + w.slice(1)).join("");
}

function generatedMethods(): Set<string> {
  const api = new GeneratedApi(new HttpClient({}));
  const names = new Set<string>();
  for (const group of [api.health, api.v1] as Array<Record<string, unknown>>) {
    for (const [name, value] of Object.entries(group)) {
      if (typeof value === "function") names.add(name);
    }
  }
  return names;
}

describe("generated client", () => {
  it("covers every contract operation", () => {
    const expected = new Set(contractOperationIds().map(camel));
    const produced = generatedMethods();
    expect([...expected].filter((n) => !produced.has(n))).toEqual([]);
    expect([...produced].filter((n) => !expected.has(n))).toEqual([]);
  });

  it("still describes 74 public operations", () => {
    // Not a magic number for its own sake: the SDK, both coverage manifests and
    // the docs all state 74, and a contract that quietly grows or shrinks should
    // make those statements fail rather than become stale.
    expect(contractOperationIds().length).toBe(74);
    expect(generatedMethods().size).toBe(74);
  });

  it("declares one @request line per operation", () => {
    expect(source.match(/@request /g)?.length).toBe(74);
  });

  it("targets the contract's method and path for every operation", () => {
    const declared = [...source.matchAll(/@request (\w+):(\S+)/g)].map(([, method, p]) => ({
      method,
      path: p.replace(/\$?\{[^}]*\}/g, "{}"),
    }));
    expect(declared.length).toBe(74);

    const raw = readFileSync(path.join(SDK_ROOT, "../../api/openapi/valori-v1.yaml"), "utf8");
    const contractPairs = new Set<string>();
    let currentPath = "";
    for (const line of raw.split("\n")) {
      const pathMatch = line.match(/^  (\/\S*):\s*$/);
      if (pathMatch) currentPath = pathMatch[1];
      const methodMatch = line.match(/^    (get|post|put|delete|patch|head|options):\s*$/);
      if (methodMatch && currentPath) {
        contractPairs.add(
          `${methodMatch[1].toUpperCase()} ${currentPath.replace(/\{[^}]*\}/g, "{}")}`,
        );
      }
    }
    for (const { method, path: p } of declared) {
      expect(contractPairs).toContain(`${method} ${p}`);
    }
  });

  it("does not import the handwritten layer", () => {
    // §4: the arrow points one way. Generated must never reach up into src/.
    expect(source).not.toMatch(/from\s+["']\.\.\/src\//);
    expect(source).not.toMatch(/require\(["']\.\.\/src\//);
  });

  it("carries no @ts-nocheck — the generated surface is typechecked", () => {
    expect(source).not.toContain("@ts-nocheck");
  });

  it("is marked as machine output", () => {
    expect(source.slice(0, 400)).toContain("GENERATED FILE — DO NOT EDIT");
  });

  it("is reachable from the handwritten client without a second HTTP stack", async () => {
    const { client, recorder } = makeClient(noContent);
    await client.raw.v1.listCollections();
    expect(recorder.count).toBe(1);
    expect(recorder.last.url.pathname).toBe("/v1/namespaces");
  });
});
