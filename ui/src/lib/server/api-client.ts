import fs from "fs";
import path from "path";
import os from "os";
import { getApiUrl } from "./connection";
import { nodeHeaders } from "./http";

// Single point of truth for "where does this request go and what auth does
// it need" — every /api/* route handler should resolve its target through
// this instead of hardcoding a URL/header shape. Two modes:
//
//   local — talks straight to whatever node the desktop app's daemon has
//   connected (getApiUrl(), unchanged from before this module existed).
//
//   cloud — talks to the Valori Cloud backend's per-project proxy
//   (VALORI_CLOUD_API_URL/v1/projects/:id/...), authenticated with the
//   caller's Supabase session, same shape valori-ui's own API routes use.
//
// The website deployment (valori-ui) is always cloud — VALORI_FORCE_CLOUD=1
// is set in its environment. The desktop app defaults to local and only
// becomes cloud after "Sign in to sync" calls setMode('cloud').

export type UiMode = "local" | "cloud";

const VALORI_HOME = process.env.VALORI_HOME || path.join(os.homedir(), ".valori");
const MODE_FILE = path.join(VALORI_HOME, "ui-mode.json");
const FORCE_CLOUD = process.env.VALORI_FORCE_CLOUD === "1";

export function getMode(): UiMode {
    if (FORCE_CLOUD) return "cloud";
    try {
        const raw = fs.readFileSync(MODE_FILE, "utf8");
        return JSON.parse(raw).mode === "cloud" ? "cloud" : "local";
    } catch {
        return "local";
    }
}

// No-op on the website deployment — mode there is fixed by
// VALORI_FORCE_CLOUD, not something a request could toggle.
export function setMode(mode: UiMode): void {
    if (FORCE_CLOUD) return;
    fs.mkdirSync(VALORI_HOME, { recursive: true });
    fs.writeFileSync(MODE_FILE, JSON.stringify({ mode }, null, 2));
}

export interface ApiTarget {
    mode: UiMode;
    /** Builds a full URL for a node-shaped subpath, e.g. "/health", "/v1/search". */
    url(subpath: string): string;
    /** Auth + content headers for the resolved mode. */
    headers(json?: boolean): Promise<Record<string, string>>;
}

/**
 * Resolves where a request should go. `projectId` is required in cloud
 * mode (there's no "current node" the way local mode has one via
 * getApiUrl()) and ignored in local mode.
 */
export async function resolveTarget(projectId?: string): Promise<ApiTarget> {
    const mode = getMode();

    if (mode === "local") {
        return {
            mode,
            url: (subpath: string) => `${getApiUrl()}${subpath}`,
            headers: async (json = true) => nodeHeaders(json),
        };
    }

    if (!projectId) {
        throw new Error("resolveTarget: projectId is required in cloud mode");
    }
    const apiUrl = process.env.VALORI_CLOUD_API_URL ?? "https://api.valori.systems";
    return {
        mode,
        url: (subpath: string) => `${apiUrl}/v1/projects/${projectId}${subpath}`,
        headers: async (json = true) => {
            const { createClient } = await import("@/utils/supabase/server");
            const supabase = await createClient();
            const {
                data: { session },
            } = await supabase.auth.getSession();
            const h: Record<string, string> = {};
            if (json) h["Content-Type"] = "application/json";
            if (session) h["Authorization"] = `Bearer ${session.access_token}`;
            return h;
        },
    };
}
