import { NextRequest, NextResponse } from "next/server";
import fs from "fs";
import path from "path";

export interface LocalFile {
  name: string;
  path: string;
  kind: "snap" | "log" | "other";
  size_bytes: number;
  modified_at: string; // ISO
  exists: boolean;
}

function statFile(filePath: string): LocalFile | null {
  try {
    const stat = fs.statSync(filePath);
    if (!stat.isFile()) return null;
    const ext = path.extname(filePath).toLowerCase();
    return {
      name: path.basename(filePath),
      path: filePath,
      kind: ext === ".snap" ? "snap" : ext === ".log" ? "log" : "other",
      size_bytes: stat.size,
      modified_at: stat.mtime.toISOString(),
      exists: true,
    };
  } catch {
    // File doesn't exist yet — still report it so the UI can show "not created yet"
    return {
      name: path.basename(filePath),
      path: filePath,
      kind: path.extname(filePath).toLowerCase() === ".snap" ? "snap" : "log",
      size_bytes: 0,
      modified_at: new Date(0).toISOString(),
      exists: false,
    };
  }
}

function scanDir(dir: string): LocalFile[] {
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }

  const results: LocalFile[] = [];
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    const ext = path.extname(entry.name).toLowerCase();
    if (ext !== ".snap" && ext !== ".log") continue;

    const full = path.join(dir, entry.name);
    let stat: fs.Stats;
    try { stat = fs.statSync(full); } catch { continue; }

    results.push({
      name: entry.name,
      path: full,
      kind: ext === ".snap" ? "snap" : "log",
      size_bytes: stat.size,
      modified_at: stat.mtime.toISOString(),
      exists: true,
    });
  }
  return results;
}

// GET /api/local-files
//   ?files=/tmp/a.log,/tmp/b.snap   → stat specific files (preferred — no directory scan)
//   ?dirs=/tmp,/var/lib/valori       → scan full directories (power user mode)
//   (no params)                      → stat only env-var configured paths
export async function GET(req: NextRequest) {
  const rawFiles = req.nextUrl.searchParams.get("files");
  const rawDirs  = req.nextUrl.searchParams.get("dirs");

  let files: LocalFile[] = [];
  const scanned: string[] = [];

  if (rawFiles) {
    // Mode 1: stat specific file paths directly
    const paths = rawFiles.split(",").map((p) => p.trim()).filter(Boolean);
    // Deduplicate
    const seen = new Set<string>();
    for (const p of paths) {
      if (seen.has(p)) continue;
      seen.add(p);
      const f = statFile(p);
      if (f) files.push(f);
    }
    scanned.push(...paths);
  } else if (rawDirs) {
    // Mode 2: scan specific directories (power user — may show unrelated files)
    const dirs = [...new Set(rawDirs.split(",").map((d) => d.trim()).filter(Boolean))];
    for (const dir of dirs) {
      scanned.push(dir);
      files.push(...scanDir(dir));
    }
  } else {
    // Mode 3: no params — read env vars and stat those exact paths only
    const snapPath = process.env.VALORI_SNAPSHOT_PATH;
    const logPath  = process.env.VALORI_EVENT_LOG_PATH;
    const raftPath = process.env.VALORI_RAFT_LOG_PATH;

    for (const p of [logPath, snapPath, raftPath]) {
      if (!p) continue;
      if (scanned.includes(p)) continue;
      scanned.push(p);
      const f = statFile(p);
      if (f) files.push(f);
    }
  }

  // Sort: snaps first, then logs; newest first within each group
  files.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === "snap" ? -1 : 1;
    return b.modified_at.localeCompare(a.modified_at);
  });

  // Deduplicate by path (in case the same file appears via two routes)
  const seen = new Set<string>();
  files = files.filter((f) => {
    if (seen.has(f.path)) return false;
    seen.add(f.path);
    return true;
  });

  return NextResponse.json({ files, scanned });
}
