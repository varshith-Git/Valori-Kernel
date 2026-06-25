"use client";

import { useState, useEffect, useCallback, useRef } from "react";

// --- Types --------------------------------------------------------------------

interface CertData {
  type: "ValoriProofCertificate";
  version: string;
  issued_at: string;            // ISO-8601
  collection: string;
  namespace: string;
  state: {
    blake3_global_hash: string | null;
    sha256_namespace_hash: string;
    record_count: number;
    node_count: number;
    global_event_count: number | null;
    ns_event_count: number;
  };
  verification: {
    method: string;
    instructions: string;
  };
  certificate_hash: string | null; // filled after signing
}

interface Baseline {
  blake3: string | null;
  ns_hash: string;
  record_count: number;
  ns_event_count: number;
  saved_at: string;
  note: string;
}

// --- Helpers ------------------------------------------------------------------

function baselineKey(namespace: string) {
  return `valori:tamper:${namespace}`;
}

function loadBaseline(namespace: string): Baseline | null {
  try {
    const raw = localStorage.getItem(baselineKey(namespace));
    return raw ? (JSON.parse(raw) as Baseline) : null;
  } catch { return null; }
}

function saveBaseline(namespace: string, b: Baseline) {
  try { localStorage.setItem(baselineKey(namespace), JSON.stringify(b)); } catch {}
}

function clearBaseline(namespace: string) {
  try { localStorage.removeItem(baselineKey(namespace)); } catch {}
}

async function sha256hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return "sha256:" + Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function shortHash(h: string | null | undefined, n = 16): string {
  if (!h) return "—";
  const core = h.startsWith("sha256:") ? h.slice(7) : h;
  return core.slice(0, n) + "…" + core.slice(-8);
}

function timeSince(iso: string): string {
  const secs = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

// --- Certificate fetch + sign -------------------------------------------------

interface AuditSnap {
  ns_proof_hash: string;
  global_state_hash: string | null;
  global_event_count: number | null;
  record_count: number;
  node_count: number;
  ns_event_ids: number[];
  error?: string;
}

async function fetchAudit(namespace: string): Promise<AuditSnap> {
  const res = await fetch(`/api/namespace-audit?namespace=${encodeURIComponent(namespace)}`, {
    cache: "no-store",
  });
  if (!res.ok) throw new Error(`Audit fetch failed (${res.status})`);
  return (await res.json()) as AuditSnap;
}

async function fetchGlobalHash(): Promise<string | null> {
  try {
    const res = await fetch("/api/proof", { cache: "no-store" });
    if (!res.ok) return null;
    const d = await res.json() as { final_state_hash?: string };
    return d.final_state_hash ?? null;
  } catch { return null; }
}

async function buildCertificate(
  collection: string,
  namespace: string,
  version: string
): Promise<{ cert: CertData; json: string }> {
  const [audit, blake3] = await Promise.all([
    fetchAudit(namespace),
    fetchGlobalHash(),
  ]);
  if (audit.error) throw new Error(audit.error);

  const payload: CertData = {
    type: "ValoriProofCertificate",
    version,
    issued_at: new Date().toISOString(),
    collection,
    namespace,
    state: {
      blake3_global_hash: blake3,
      sha256_namespace_hash: audit.ns_proof_hash,
      record_count: audit.record_count,
      node_count: audit.node_count,
      global_event_count: audit.global_event_count,
      ns_event_count: audit.ns_event_ids.length,
    },
    verification: {
      method: "SHA-256 self-certification",
      instructions:
        "Replay events.log through valori-verify and compare final_state_hash. " +
        "The namespace hash is SHA-256 of the sorted list of event IDs touching this collection. " +
        "The certificate_hash is SHA-256 of this document with certificate_hash set to null.",
    },
    certificate_hash: null,
  };

  const canonical = JSON.stringify(payload);
  const certHash = await sha256hex(canonical);
  const signed: CertData = { ...payload, certificate_hash: certHash };
  const json = JSON.stringify(signed, null, 2);
  return { cert: signed, json };
}

// --- Print popup --------------------------------------------------------------

function printCertificate(cert: CertData) {
  const w = window.open("", "_blank", "width=860,height=900");
  if (!w) { alert("Allow popups to print the certificate."); return; }

  const fmt = new Intl.DateTimeFormat(undefined, {
    dateStyle: "long", timeStyle: "medium",
  }).format(new Date(cert.issued_at));

  const row = (label: string, value: string) =>
    `<tr><td class="lbl">${label}</td><td class="val">${value}</td></tr>`;

  const hashBox = (label: string, value: string | null) => `
    <div class="hl">${label}</div>
    <div class="hb">${value ?? "(unavailable)"}</div>`;

  w.document.write(`<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<title>Valori Proof Certificate</title>
<style>
  @page{margin:18mm;size:A4}
  *{box-sizing:border-box;margin:0;padding:0}
  body{font-family:'Courier New',Courier,monospace;color:#111;background:#fff;font-size:12px;line-height:1.5}
  .wrap{border:2px solid #111;padding:36px;min-height:240mm;display:flex;flex-direction:column;gap:0}
  .top{display:flex;justify-content:space-between;align-items:flex-start;border-bottom:1px solid #aaa;padding-bottom:18px;margin-bottom:24px}
  .brand{font-size:20px;font-weight:bold;letter-spacing:3px}
  .brand-sub{font-size:10px;color:#555;letter-spacing:1px;margin-top:3px}
  .meta{text-align:right;font-size:11px;color:#555}
  .cert-title{text-align:center;font-size:14px;letter-spacing:5px;text-transform:uppercase;margin-bottom:24px;border-bottom:1px solid #ddd;padding-bottom:12px}
  table{width:100%;border-collapse:collapse;margin-bottom:20px}
  td{padding:5px 4px;vertical-align:top}
  td.lbl{width:180px;font-size:10px;text-transform:uppercase;letter-spacing:1px;color:#666;padding-top:7px}
  td.val{font-size:13px;font-weight:bold}
  .hl{font-size:9px;text-transform:uppercase;letter-spacing:1.5px;color:#666;margin-bottom:4px;margin-top:14px}
  .hb{border:1px solid #bbb;padding:10px 12px;font-size:10.5px;word-break:break-all;background:#f7f7f7;letter-spacing:.5px}
  .fp{border:2px solid #111;padding:14px;text-align:center;font-size:11px;word-break:break-all;margin:20px 0;background:#f0f0f0}
  .fp-lbl{font-size:9px;letter-spacing:2px;text-transform:uppercase;color:#555;margin-bottom:6px}
  .note{font-size:9.5px;color:#555;line-height:1.7;margin-top:auto;padding-top:16px;border-top:1px solid #ddd}
  .foot{display:flex;justify-content:space-between;font-size:9px;color:#888;margin-top:10px}
  @media print{body{-webkit-print-color-adjust:exact;print-color-adjust:exact}}
</style>
</head>
<body>
<div class="wrap">
  <div class="top">
    <div>
      <div class="brand">VALORI</div>
      <div class="brand-sub">KERNEL · TAMPER-EVIDENT VECTOR STORE</div>
    </div>
    <div class="meta">
      <div>${fmt}</div>
      <div>v${cert.version}</div>
    </div>
  </div>

  <div class="cert-title">Proof Certificate</div>

  <table>
    ${row("Collection", cert.collection)}
    ${row("Namespace", cert.namespace)}
    ${row("Records", cert.state.record_count.toLocaleString())}
    ${row("Graph nodes", cert.state.node_count.toLocaleString())}
    ${row("Global events", cert.state.global_event_count?.toLocaleString() ?? "—")}
    ${row("Namespace events", cert.state.ns_event_count.toLocaleString())}
  </table>

  ${hashBox("BLAKE3 Global State Hash", cert.state.blake3_global_hash)}
  ${hashBox("SHA-256 Namespace Proof Hash", cert.state.sha256_namespace_hash)}

  <div class="fp-lbl">Certificate Fingerprint &nbsp;(SHA-256 of payload)</div>
  <div class="fp">${cert.certificate_hash}</div>

  <div class="note">
    <strong>To verify independently:</strong> Replay events.log through
    <code>valori-verify</code> and compare the <code>final_state_hash</code> field against the
    BLAKE3 hash above. The namespace proof hash is the SHA-256 digest of the sorted list of
    event IDs that touch records or nodes in this collection — reproducible from any copy of the
    event log. The certificate fingerprint is SHA-256 of this document with the fingerprint field
    set to null.
  </div>

  <div class="foot">
    <span>Valori Kernel · deterministic · tamper-evident · Q16.16 fixed-point</span>
    <span>ID: ${(cert.certificate_hash ?? "").slice(7, 15).toUpperCase()}</span>
  </div>
</div>
</body>
</html>`);
  w.document.close();
  w.focus();
  setTimeout(() => { w.print(); }, 400);
}

// --- Copy button --------------------------------------------------------------

function CopyBtn({ text, label = "copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  const copy = useCallback(async () => {
    await navigator.clipboard.writeText(text);
    setDone(true);
    setTimeout(() => setDone(false), 1500);
  }, [text]);
  return (
    <button
      onClick={copy}
      className={`text-xs px-3 py-1.5 rounded border transition-all ${
        done
          ? "border-emerald-700 bg-emerald-950/40 text-emerald-400"
          : "border-input text-muted-foreground hover:text-foreground hover:border-ring bg-card"
      }`}
    >
      {done ? "✓ copied" : label}
    </button>
  );
}

// --- Certificate section ------------------------------------------------------

function CertSection({
  collection,
  namespace,
}: {
  collection: string;
  namespace: string;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{ cert: CertData; json: string } | null>(null);
  const [view, setView] = useState<"visual" | "json">("visual");

  const generate = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      // Detect version from health
      const h = await fetch("/api/health", { cache: "no-store" }).then((r) =>
        r.ok ? r.json() : {}
      ) as { version?: string };
      const res = await buildCertificate(collection, namespace, h.version ?? "unknown");
      setResult(res);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to generate certificate");
    } finally {
      setBusy(false);
    }
  }, [collection, namespace]);

  const downloadJSON = useCallback(() => {
    if (!result) return;
    const blob = new Blob([result.json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `valori-cert-${namespace.replace(/[^a-z0-9]/gi, "-")}-${
      Date.now()
    }.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, [result, namespace]);

  const cert = result?.cert;

  return (
    <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-5">
      <div className="flex items-start justify-between">
        <div>
          <p className="text-sm font-semibold text-card-foreground">Proof Certificate</p>
          <p className="text-xs text-muted-foreground mt-0.5">
            Signed JSON + printable PDF containing namespace hash, global BLAKE3 state hash,
            record counts, and a SHA-256 self-certification fingerprint.
          </p>
        </div>
        <button
          onClick={generate}
          disabled={busy}
          className="flex-shrink-0 text-sm px-4 py-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 font-medium disabled:opacity-50 transition-colors ml-4"
        >
          {busy ? "Generating…" : result ? "Regenerate" : "Generate →"}
        </button>
      </div>

      {error && (
        <p className="text-xs text-red-400 font-mono">{error}</p>
      )}

      {cert && result && (
        <>
          {/* Tab toggle */}
          <div className="flex items-center gap-0.5 bg-accent rounded-md border border-input p-0.5 w-fit">
            {(["visual", "json"] as const).map((v) => (
              <button
                key={v}
                onClick={() => setView(v)}
                className={`px-3 py-1 text-xs rounded transition-colors ${
                  view === v
                    ? "bg-muted text-foreground"
                    : "text-muted-foreground hover:text-accent-foreground"
                }`}
              >
                {v === "visual" ? "Certificate" : "JSON"}
              </button>
            ))}
          </div>

          {view === "json" ? (
            <div className="relative rounded-lg bg-background border border-border overflow-hidden">
              <pre className="text-[11.5px] font-mono text-accent-foreground p-4 overflow-x-auto leading-relaxed">
                {result.json}
              </pre>
              <div className="absolute top-2 right-2 flex gap-1.5">
                <CopyBtn text={result.json} label="copy JSON" />
                <button
                  onClick={downloadJSON}
                  className="text-xs px-3 py-1.5 rounded border border-input text-muted-foreground hover:text-foreground hover:border-ring bg-card transition-all"
                >
                  download
                </button>
              </div>
            </div>
          ) : (
            /* Visual certificate preview */
            <div className="rounded-xl border-2 border-input bg-background overflow-hidden">
              {/* Header */}
              <div className="flex items-start justify-between px-6 py-4 border-b border-border">
                <div>
                  <p className="font-mono text-xs font-bold tracking-[4px] text-card-foreground">VALORI</p>
                  <p className="font-mono text-[9px] text-muted-foreground tracking-widest mt-0.5">
                    KERNEL · TAMPER-EVIDENT VECTOR STORE
                  </p>
                </div>
                <div className="text-right">
                  <p className="text-[10px] text-muted-foreground font-mono">
                    {new Date(cert.issued_at).toLocaleString()}
                  </p>
                  <p className="text-[10px] text-muted-foreground font-mono">v{cert.version}</p>
                </div>
              </div>

              <div className="px-6 py-5 flex flex-col gap-4">
                <p className="text-center font-mono text-xs tracking-[6px] text-muted-foreground uppercase border-b border-border pb-4">
                  Proof Certificate
                </p>

                {/* Data grid */}
                <div className="grid grid-cols-2 gap-x-6 gap-y-2 text-xs">
                  {[
                    ["Collection", cert.collection],
                    ["Namespace", cert.namespace],
                    ["Records", cert.state.record_count.toLocaleString()],
                    ["Graph nodes", cert.state.node_count.toLocaleString()],
                    ["Global events", cert.state.global_event_count?.toLocaleString() ?? "—"],
                    ["Namespace events", cert.state.ns_event_count.toLocaleString()],
                  ].map(([label, value]) => (
                    <div key={label} className="flex flex-col gap-0.5">
                      <span className="text-[9px] text-muted-foreground uppercase tracking-widest">{label}</span>
                      <span className="font-mono text-accent-foreground font-semibold truncate">{value}</span>
                    </div>
                  ))}
                </div>

                {/* Hash fields */}
                <div className="flex flex-col gap-3">
                  {[
                    { label: "BLAKE3 Global State Hash", value: cert.state.blake3_global_hash },
                    { label: "SHA-256 Namespace Proof Hash", value: cert.state.sha256_namespace_hash },
                  ].map(({ label, value }) => (
                    <div key={label}>
                      <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">{label}</p>
                      <div className="rounded border border-border bg-card px-3 py-2 font-mono text-[11px] text-muted-foreground break-all flex items-start justify-between gap-2">
                        <span className="flex-1 break-all">{value ?? "unavailable"}</span>
                        {value && <CopyBtn text={value} label="copy" />}
                      </div>
                    </div>
                  ))}
                </div>

                {/* Certificate fingerprint */}
                <div className="rounded-lg border-2 border-input bg-card px-4 py-3">
                  <p className="text-[9px] text-muted-foreground uppercase tracking-[3px] mb-2 text-center">
                    Certificate Fingerprint (SHA-256 of payload)
                  </p>
                  <p className="font-mono text-[10.5px] text-accent-foreground break-all text-center">
                    {cert.certificate_hash}
                  </p>
                </div>
              </div>

              {/* Footer */}
              <div className="px-6 py-3 border-t border-border bg-background/80 flex items-center justify-between">
                <p className="text-[9px] text-muted-foreground font-mono">
                  Valori Kernel · deterministic · Q16.16 · BLAKE3-chained
                </p>
                <p className="text-[9px] text-muted-foreground font-mono">
                  ID: {(cert.certificate_hash ?? "").slice(7, 15).toUpperCase()}
                </p>
              </div>
            </div>
          )}

          {/* Action buttons */}
          <div className="flex items-center gap-2 flex-wrap">
            <button
              onClick={() => printCertificate(cert)}
              className="text-sm px-4 py-2 rounded-lg border border-input text-accent-foreground hover:text-foreground hover:border-ring transition-colors"
            >
              🖨 Print / Save as PDF
            </button>
            <CopyBtn text={result.json} label="copy JSON" />
            <button
              onClick={downloadJSON}
              className="text-xs px-3 py-1.5 rounded border border-input text-muted-foreground hover:text-foreground hover:border-ring bg-card transition-all"
            >
              download .json
            </button>
          </div>
        </>
      )}
    </div>
  );
}

// --- Tamper detection section -------------------------------------------------

function TamperSection({ namespace }: { namespace: string }) {
  const [baseline, setBaseline] = useState<Baseline | null>(() =>
    loadBaseline(namespace)
  );
  const [current, setCurrent] = useState<{
    blake3: string | null;
    ns_hash: string | null;
    record_count: number | null;
    ns_event_count: number | null;
    fetched_at: string;
  } | null>(null);
  const [loading, setLoading] = useState(false);
  const [baselineNote, setBaselineNote] = useState("");
  const [noteInput, setNoteInput] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchCurrent = useCallback(async () => {
    setLoading(true);
    try {
      const [audit, blake3] = await Promise.all([
        fetchAudit(namespace).catch(() => null),
        fetchGlobalHash(),
      ]);
      setCurrent({
        blake3,
        ns_hash: audit?.ns_proof_hash ?? null,
        record_count: audit?.record_count ?? null,
        ns_event_count: audit?.ns_event_ids.length ?? null,
        fetched_at: new Date().toISOString(),
      });
    } finally {
      setLoading(false);
    }
  }, [namespace]);

  // Poll every 5s
  useEffect(() => {
    fetchCurrent();
    intervalRef.current = setInterval(fetchCurrent, 5000);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [fetchCurrent]);

  // Re-load baseline when namespace changes
  useEffect(() => {
    setBaseline(loadBaseline(namespace));
  }, [namespace]);

  const saveAsBaseline = useCallback(() => {
    if (!current?.ns_hash) return;
    const b: Baseline = {
      blake3: current.blake3,
      ns_hash: current.ns_hash,
      record_count: current.record_count ?? 0,
      ns_event_count: current.ns_event_count ?? 0,
      saved_at: new Date().toISOString(),
      note: baselineNote.trim(),
    };
    saveBaseline(namespace, b);
    setBaseline(b);
    setBaselineNote("");
    setNoteInput(false);
  }, [current, namespace, baselineNote]);

  // Determine status
  type TamperStatus = "no-baseline" | "loading" | "match" | "mismatch";
  let status: TamperStatus = "no-baseline";
  let detailMsg = "";

  if (baseline && current) {
    if (current.ns_hash === null) {
      status = "loading";
    } else if (current.ns_hash === baseline.ns_hash) {
      status = "match";
      detailMsg = `Namespace state identical to baseline saved ${timeSince(baseline.saved_at)}`;
      if (baseline.blake3 && current.blake3 && baseline.blake3 !== current.blake3) {
        status = "mismatch";
        detailMsg = "Namespace hash matches but global state has changed — another namespace was modified";
      }
    } else {
      status = "mismatch";
      detailMsg = `Namespace proof hash changed since baseline saved ${timeSince(baseline.saved_at)}`;
    }
  } else if (!baseline) {
    status = "no-baseline";
  } else {
    status = "loading";
  }

  return (
    <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-5">
      <div>
        <p className="text-sm font-semibold text-card-foreground">Tamper Detection</p>
        <p className="text-xs text-muted-foreground mt-0.5">
          Save a baseline snapshot of the namespace proof hash, then compare it to the live state at any time.
        </p>
      </div>

      {/* Two-column: baseline + current */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">

        {/* Baseline */}
        <div className="rounded-lg border border-border bg-background p-4 flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <p className="text-xs font-medium text-muted-foreground uppercase tracking-widest">Baseline</p>
            {baseline && (
              <button
                onClick={() => { clearBaseline(namespace); setBaseline(null); }}
                className="text-[10px] text-muted-foreground hover:text-red-400 transition-colors"
              >
                clear
              </button>
            )}
          </div>
          {baseline ? (
            <div className="flex flex-col gap-2">
              <div>
                <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">Namespace Hash</p>
                <p className="font-mono text-[10px] text-muted-foreground break-all">{shortHash(baseline.ns_hash, 24)}</p>
              </div>
              <div>
                <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">BLAKE3 Global</p>
                <p className="font-mono text-[10px] text-muted-foreground break-all">{shortHash(baseline.blake3, 24)}</p>
              </div>
              <div className="flex gap-4 text-[10px] text-muted-foreground">
                <span>{baseline.record_count.toLocaleString()} records</span>
                <span>{baseline.ns_event_count.toLocaleString()} ns events</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-[10px] text-muted-foreground font-mono">{timeSince(baseline.saved_at)}</span>
                {baseline.note && (
                  <span className="text-[10px] text-muted-foreground italic truncate">{baseline.note}</span>
                )}
              </div>
            </div>
          ) : (
            <p className="text-xs text-muted-foreground italic">No baseline saved</p>
          )}

          {/* Save note + button */}
          <div className="flex flex-col gap-2 mt-auto pt-2 border-t border-border">
            {noteInput && (
              <input
                type="text"
                value={baselineNote}
                onChange={(e) => setBaselineNote(e.target.value)}
                placeholder="Optional note (e.g. before migration)"
                className="text-xs bg-accent border border-input rounded px-2 py-1.5 text-accent-foreground placeholder:text-muted-foreground focus:outline-none focus:border-ring"
              />
            )}
            <div className="flex gap-2">
              <button
                onClick={saveAsBaseline}
                disabled={!current?.ns_hash || loading}
                className="flex-1 text-xs py-1.5 rounded bg-muted text-foreground hover:bg-muted disabled:opacity-40 transition-colors"
              >
                {baseline ? "Update baseline" : "Save as baseline"}
              </button>
              <button
                onClick={() => setNoteInput((v) => !v)}
                className="text-xs px-2 py-1.5 rounded border border-input text-muted-foreground hover:text-accent-foreground transition-colors"
                title="Add a note"
              >
                ✎
              </button>
            </div>
          </div>
        </div>

        {/* Current */}
        <div className="rounded-lg border border-border bg-background p-4 flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <p className="text-xs font-medium text-muted-foreground uppercase tracking-widest">Live State</p>
            <span className="text-[10px] text-muted-foreground font-mono flex items-center gap-1">
              {loading && <span className="animate-spin inline-block">⟳</span>}
              {current
                ? `updated ${timeSince(current.fetched_at)}`
                : "fetching…"}
            </span>
          </div>
          {current ? (
            <div className="flex flex-col gap-2">
              <div>
                <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">Namespace Hash</p>
                <p className="font-mono text-[10px] text-muted-foreground break-all">{shortHash(current.ns_hash, 24)}</p>
              </div>
              <div>
                <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">BLAKE3 Global</p>
                <p className="font-mono text-[10px] text-muted-foreground break-all">{shortHash(current.blake3, 24)}</p>
              </div>
              {current.record_count !== null && (
                <div className="flex gap-4 text-[10px] text-muted-foreground">
                  <span>{current.record_count.toLocaleString()} records</span>
                  <span>{current.ns_event_count?.toLocaleString() ?? "—"} ns events</span>
                </div>
              )}
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              <div className="h-4 bg-accent rounded animate-pulse" />
              <div className="h-4 bg-accent rounded animate-pulse w-3/4" />
            </div>
          )}
        </div>
      </div>

      {/* Status banner */}
      {status === "no-baseline" && (
        <div className="rounded-xl border border-input bg-accent/50 px-5 py-4 flex items-center gap-4">
          <span className="text-2xl text-muted-foreground">◌</span>
          <div>
            <p className="text-sm font-medium text-muted-foreground">No baseline set</p>
            <p className="text-xs text-muted-foreground mt-0.5">
              Save the current state as a baseline to enable tamper detection.
            </p>
          </div>
        </div>
      )}

      {status === "loading" && (
        <div className="rounded-xl border border-input bg-accent/50 px-5 py-4 flex items-center gap-4">
          <span className="text-2xl text-muted-foreground animate-spin">⟳</span>
          <p className="text-sm text-muted-foreground">Fetching current state…</p>
        </div>
      )}

      {status === "match" && (
        <div className="rounded-xl border-2 border-emerald-800 bg-emerald-950/30 px-5 py-5 flex items-start gap-4">
          <span className="text-3xl text-emerald-400 flex-shrink-0">✓</span>
          <div>
            <p className="text-base font-bold text-emerald-400 tracking-wide">HASH MATCH</p>
            <p className="text-xs text-emerald-700 mt-1">{detailMsg}</p>
            <p className="text-[10px] text-muted-foreground font-mono mt-2">
              ns: <span className="text-muted-foreground">{shortHash(current?.ns_hash, 20)}</span>
            </p>
          </div>
        </div>
      )}

      {status === "mismatch" && (
        <div className="rounded-xl border-2 border-red-800 bg-red-950/30 px-5 py-5 flex flex-col gap-3">
          <div className="flex items-start gap-4">
            <span className="text-3xl text-red-400 flex-shrink-0">✗</span>
            <div>
              <p className="text-base font-bold text-red-400 tracking-wide">HASH MISMATCH</p>
              <p className="text-xs text-red-700 mt-1">{detailMsg}</p>
            </div>
          </div>
          <div className="rounded-lg bg-background border border-red-900/40 px-4 py-3 font-mono text-[10px] flex flex-col gap-2">
            <div className="flex gap-2 items-start">
              <span className="text-muted-foreground w-20 flex-shrink-0">Baseline</span>
              <span className="text-muted-foreground break-all">{shortHash(baseline?.ns_hash, 32)}</span>
            </div>
            <div className="flex gap-2 items-start">
              <span className="text-muted-foreground w-20 flex-shrink-0">Current</span>
              <span className="text-red-400 break-all">{shortHash(current?.ns_hash, 32)}</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// --- Main tab -----------------------------------------------------------------

export function CertifyTab({
  namespace,
  collection,
}: {
  namespace: string;
  collection: string;
}) {
  return (
    <div className="flex flex-col gap-6 max-w-3xl">
      <CertSection collection={collection} namespace={namespace} />
      <TamperSection namespace={namespace} />
    </div>
  );
}
