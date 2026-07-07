import fs from "fs";
import path from "path";
import os from "os";

export interface SavedConnection {
  url:           string;
  lastConnected: string;          // ISO date
  dim?:          number;
  records?:      number;
  status?:       string;
}

const HISTORY_FILE = path.join(os.homedir(), ".valori", "ui-connections.json");
const MAX_HISTORY  = 8;

function readHistory(): SavedConnection[] {
  try { return JSON.parse(fs.readFileSync(HISTORY_FILE, "utf8")) as SavedConnection[]; }
  catch { return []; }
}

function writeHistory(list: SavedConnection[]): void {
  try {
    fs.mkdirSync(path.dirname(HISTORY_FILE), { recursive: true });
    fs.writeFileSync(HISTORY_FILE, JSON.stringify(list, null, 2));
  } catch {}
}

function pushHistory(url: string, info?: Pick<SavedConnection, "dim" | "records" | "status">): void {
  const list = readHistory().filter(h => h.url !== url);
  list.unshift({ url, lastConnected: new Date().toISOString(), ...info });
  writeHistory(list.slice(0, MAX_HISTORY));
}

declare global { var __valori_conn_url__: string | undefined; }

// On cold start, if no VALORI_API_URL env var, restore the last saved URL.
if (!global.__valori_conn_url__ && !process.env.VALORI_API_URL) {
  const last = readHistory()[0];
  if (last) global.__valori_conn_url__ = last.url;
}

export function getApiUrl(): string {
  if (process.env.VALORI_API_URL) return process.env.VALORI_API_URL;
  
  // Read from history to support multi-worker Next.js dev server state sync
  // and avoid stale global variables when switching projects.
  const last = readHistory()[0];
  if (last) {
    return last.url;
  }
  return "http://127.0.0.1:3000";
}

export function setApiUrl(url: string, info?: Pick<SavedConnection, "dim" | "records" | "status">): void {
  const clean = url.replace(/\/+$/, "");
  global.__valori_conn_url__ = clean;
  pushHistory(clean, info);
}

export function resetApiUrl(): void {
  global.__valori_conn_url__ = undefined;
}

export function getHistory(): SavedConnection[] {
  return readHistory();
}

export function removeUrlFromHistory(url: string): void {
  const clean = url.replace(/\/+$/, "");
  const list = readHistory().filter(h => h.url !== clean);
  writeHistory(list);
  
  if (global.__valori_conn_url__ === clean) {
    resetApiUrl();
    const next = list[0];
    if (next && !process.env.VALORI_API_URL) {
      global.__valori_conn_url__ = next.url;
    }
  }
}
