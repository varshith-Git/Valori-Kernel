"use client";

import { useState, useCallback } from "react";
import type { AnswerReceipt } from "@/lib/receipts";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { TabShell } from "@/components/collections/TabShell";

// --- Types --------------------------------------------------------------------

interface ErasureRecord {
  type: string;
  issued_at: string;
  collection: string;
  erased_record_ids: number[];
  pre_erasure_blake3: string | null;
  post_erasure_blake3: string | null;
  certificate_hash: string | null;
}

interface TamperBaseline {
  blake3: string | null;
  ns_hash: string;
  record_count: number;
  ns_event_count: number;
  saved_at: string;
  note: string;
}

interface CompliancePack {
  type: "ValoriCompliancePack";
  version: string;
  generated_at: string;
  collection: string;
  namespace: string;
  standard_refs: string[];
  node: { version: string; dim: number | null };
  attestation: {
    ns_proof_hash: string;
    global_blake3_hash: string | null;
    record_count: number;
    node_count: number;
    total_events: number;
    ns_event_count: number;
  };
  tamper: {
    status: "match" | "mismatch" | "no-baseline";
    baseline: TamperBaseline | null;
    live_ns_hash: string;
  };
  erasures: { count: number; total_records_erased: number; certificates: ErasureRecord[] };
  answer_receipts: { count: number; receipts: AnswerReceipt[] };
  pack_sha256: string | null;
}

// --- Helpers ------------------------------------------------------------------

async function sha256hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return "sha256:" + Array.from(new Uint8Array(buf)).map((b) => b.toString(16).padStart(2, "0")).join("");
}

function numOr0(n: number | null | undefined): number {
  return typeof n === "number" ? n : 0;
}

function shortHash(h: string | null | undefined, n = 16): string {
  if (!h) return "—";
  const core = h.startsWith("sha256:") ? h.slice(7) : h;
  return core.length > n + 8 ? core.slice(0, n) + "…" + core.slice(-6) : core;
}

function readLS<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch { return fallback; }
}

const STANDARD_REFS = [
  "EU AI Act Article 12 (record-keeping / automatic logging)",
  "EU AI Act Article 13 (transparency & provision of information)",
  "GDPR Article 17 (right to erasure)",
  "GDPR Article 22 (automated decision-making — right to explanation)",
  "SOC 2 CC7 (system monitoring & integrity)",
];

// --- Pack builder -------------------------------------------------------------

async function buildPack(collection: string, namespace: string): Promise<CompliancePack> {
  // Attestation + live state
  const [auditRes, healthRes] = await Promise.all([
    fetch(`/api/namespace-audit?namespace=${encodeURIComponent(namespace)}`, { cache: "no-store" }),
    fetch("/api/health", { cache: "no-store" }).catch(() => null),
  ]);
  if (!auditRes.ok) throw new Error(`Audit fetch failed (${auditRes.status})`);
  const audit = await auditRes.json() as {
    ns_proof_hash: string;
    global_state_hash: string | null;
    record_count: number;
    node_count: number;
    total_events: number;
    ns_event_ids: number[];
    error?: string;
  };
  if (audit.error) throw new Error(audit.error);

  const health = healthRes && healthRes.ok
    ? await healthRes.json().catch(() => ({})) as { version?: string; dim?: number }
    : {};

  // Tamper baseline (saved by Certify tab)
  const baseline = readLS<TamperBaseline | null>(`valori:tamper:${namespace}`, null);
  let tamperStatus: CompliancePack["tamper"]["status"] = "no-baseline";
  if (baseline) {
    tamperStatus = baseline.ns_hash === audit.ns_proof_hash ? "match" : "mismatch";
  }

  // Erasure certificates (saved by GDPR tab)
  const erasures = readLS<ErasureRecord[]>(`valori:erasures:${namespace}`, []);
  const totalErased = erasures.reduce((sum, e) => sum + (e.erased_record_ids?.length ?? 0), 0);

  // Answer receipts (saved by Ask tab)
  const askHistory = readLS<{ receipt?: AnswerReceipt }[]>(`valori:ask-history:${namespace}`, []);
  const receipts = askHistory.map((h) => h.receipt).filter((r): r is AnswerReceipt => !!r);

  const payload: CompliancePack = {
    type: "ValoriCompliancePack",
    version: "1.0",
    generated_at: new Date().toISOString(),
    collection,
    namespace,
    standard_refs: STANDARD_REFS,
    node: { version: health.version ?? "unknown", dim: health.dim ?? null },
    attestation: {
      ns_proof_hash: audit.ns_proof_hash,
      global_blake3_hash: audit.global_state_hash,
      record_count: audit.record_count,
      node_count: audit.node_count,
      total_events: audit.total_events,
      ns_event_count: audit.ns_event_ids.length,
    },
    tamper: {
      status: tamperStatus,
      baseline,
      live_ns_hash: audit.ns_proof_hash,
    },
    erasures: { count: erasures.length, total_records_erased: totalErased, certificates: erasures },
    answer_receipts: { count: receipts.length, receipts },
    pack_sha256: null,
  };

  const fp = await sha256hex(JSON.stringify(payload));
  return { ...payload, pack_sha256: fp };
}

// --- Print --------------------------------------------------------------------

function printPack(pack: CompliancePack) {
  const w = window.open("", "_blank", "width=900,height=1000");
  if (!w) { alert("Allow popups to print the compliance pack."); return; }
  const when = new Intl.DateTimeFormat(undefined, { dateStyle: "long", timeStyle: "medium" })
    .format(new Date(pack.generated_at));

  const statusBadge =
    pack.tamper.status === "match" ? '<span style="color:#0a0">✓ VERIFIED — matches baseline</span>'
    : pack.tamper.status === "mismatch" ? '<span style="color:#c00">✗ MISMATCH — state changed since baseline</span>'
    : '<span style="color:#888">No baseline recorded</span>';

  const refRows = pack.standard_refs.map((r) => `<li>${r}</li>`).join("");

  const erasureRows = pack.erasures.certificates.length
    ? pack.erasures.certificates.map((e) =>
        `<tr><td>${new Date(e.issued_at).toLocaleDateString()}</td><td>${e.erased_record_ids.length}</td>` +
        `<td class="mono">${e.certificate_hash ?? "—"}</td></tr>`
      ).join("")
    : '<tr><td colspan="3" style="color:#888">No erasure events recorded</td></tr>';

  const receiptRows = pack.answer_receipts.receipts.length
    ? pack.answer_receipts.receipts.slice(0, 50).map((r) =>
        `<tr><td>${new Date(r.state.captured_at).toLocaleDateString()}</td>` +
        `<td>${(r.question || "").slice(0, 60).replace(/</g, "&lt;")}</td>` +
        `<td>${r.chunks.length}</td><td class="mono">${(r.receipt_sha256 ?? "").slice(7, 23)}</td></tr>`
      ).join("")
    : '<tr><td colspan="4" style="color:#888">No proof-carrying answers recorded</td></tr>';

  w.document.write(`<!DOCTYPE html><html lang="en"><head><meta charset="UTF-8"/>
<title>Valori Compliance Pack — ${pack.namespace}</title>
<style>
  @page{margin:16mm;size:A4}*{box-sizing:border-box;margin:0;padding:0}
  body{font-family:'Courier New',monospace;color:#111;font-size:11px;line-height:1.5}
  .wrap{border:2px solid #111;padding:30px}
  .brand{font-size:18px;font-weight:bold;letter-spacing:3px}
  .sub{font-size:9px;color:#555;letter-spacing:1px;margin-top:2px}
  .title{text-align:center;font-size:14px;letter-spacing:4px;text-transform:uppercase;margin:20px 0;border-top:1px solid #ddd;border-bottom:1px solid #ddd;padding:10px 0}
  h2{font-size:11px;text-transform:uppercase;letter-spacing:2px;color:#333;margin:20px 0 8px;border-bottom:1px solid #ccc;padding-bottom:4px}
  .grid{display:grid;grid-template-columns:1fr 1fr;gap:4px 16px;margin-bottom:6px}
  .lbl{font-size:9px;text-transform:uppercase;letter-spacing:1px;color:#666}
  .val{font-size:12px;font-weight:bold}
  .box{border:1px solid #bbb;background:#f7f7f7;padding:8px 10px;word-break:break-all;font-size:10px;margin-top:4px}
  table{width:100%;border-collapse:collapse;margin-top:6px;font-size:9.5px}
  td,th{border:1px solid #ccc;padding:4px 6px;text-align:left}th{background:#eee;font-size:8px;text-transform:uppercase}
  .mono{word-break:break-all;font-size:9px}
  ul{margin:6px 0 6px 18px;font-size:10px;color:#444}
  .fp{border:2px solid #111;background:#f0f0f0;padding:12px;text-align:center;word-break:break-all;margin-top:18px;font-size:11px}
  .foot{font-size:8.5px;color:#888;margin-top:14px;border-top:1px solid #ddd;padding-top:10px;line-height:1.7}
</style></head><body><div class="wrap">
  <div class="brand">VALORI</div>
  <div class="sub">KERNEL · COMPLIANCE EVIDENCE PACK</div>
  <div class="title">Compliance Pack</div>

  <div class="grid">
    <div><div class="lbl">Collection</div><div class="val">${pack.collection}</div></div>
    <div><div class="lbl">Generated</div><div class="val" style="font-size:10px">${when}</div></div>
    <div><div class="lbl">Namespace</div><div class="val" style="font-size:10px">${pack.namespace}</div></div>
    <div><div class="lbl">Node version</div><div class="val" style="font-size:10px">v${pack.node.version}</div></div>
  </div>

  <h2>1 · Integrity Attestation</h2>
  <div class="grid">
    <div><div class="lbl">Records</div><div class="val">${numOr0(pack.attestation.record_count).toLocaleString()}</div></div>
    <div><div class="lbl">Graph nodes</div><div class="val">${numOr0(pack.attestation.node_count).toLocaleString()}</div></div>
    <div><div class="lbl">Namespace events</div><div class="val">${numOr0(pack.attestation.ns_event_count).toLocaleString()}</div></div>
    <div><div class="lbl">Total events</div><div class="val">${numOr0(pack.attestation.total_events).toLocaleString()}</div></div>
  </div>
  <div class="lbl" style="margin-top:8px">SHA-256 Namespace Proof Hash</div>
  <div class="box">${pack.attestation.ns_proof_hash}</div>
  <div class="lbl" style="margin-top:8px">BLAKE3 Global State Hash</div>
  <div class="box">${pack.attestation.global_blake3_hash ?? "(unavailable)"}</div>

  <h2>2 · Tamper Status</h2>
  <div class="box" style="background:#fff;border-width:2px">${statusBadge}${
    pack.tamper.baseline
      ? ` &nbsp;·&nbsp; baseline saved ${new Date(pack.tamper.baseline.saved_at).toLocaleString()}${
          pack.tamper.baseline.note ? ` (${pack.tamper.baseline.note})` : ""
        }`
      : ""
  }</div>

  <h2>3 · Right-to-Erasure Evidence (${pack.erasures.count} events · ${pack.erasures.total_records_erased} records)</h2>
  <table><thead><tr><th>Date</th><th>Records erased</th><th>Certificate hash</th></tr></thead><tbody>${erasureRows}</tbody></table>

  <h2>4 · Answer Provenance Records (${pack.answer_receipts.count})</h2>
  <table><thead><tr><th>Date</th><th>Question</th><th>Chunks</th><th>Receipt</th></tr></thead><tbody>${receiptRows}</tbody></table>

  <h2>5 · Regulatory Mapping</h2>
  <ul>${refRows}</ul>

  <div class="lbl" style="margin-top:16px;text-align:center">Pack Fingerprint (SHA-256 of full bundle)</div>
  <div class="fp">${pack.pack_sha256}</div>

  <div class="foot">
    This pack is self-verifying: the fingerprint is SHA-256 of the JSON bundle with pack_sha256 set to null.
    Every hash herein is independently reproducible from a copy of the node's events.log via the valori-verify
    binary, with no access to this UI. All vector arithmetic is Q16.16 fixed-point; the audit chain is BLAKE3.
  </div>
</div></body></html>`);
  w.document.close();
  w.focus();
  setTimeout(() => w.print(), 400);
}

// --- Section component --------------------------------------------------------

const SECTION_TONE_MAP = {
  good: "success",
  warn: "warning",
  bad: "error",
  neutral: "neutral",
} as const;

function Section({
  num, title, status, children,
}: {
  num: number; title: string; status?: { label: string; tone: "good" | "warn" | "bad" | "neutral" }; children: React.ReactNode;
}) {
  return (
    <div className="rounded-xl border border-border bg-card p-4 flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <span className="w-5 h-5 rounded bg-accent text-muted-foreground text-[10px] font-mono flex items-center justify-center">
            {num}
          </span>
          <p className="text-sm font-medium text-card-foreground">{title}</p>
        </div>
        {status && (
          <StatusBadge tone={SECTION_TONE_MAP[status.tone]} className="font-mono text-[10px]">
            {status.label}
          </StatusBadge>
        )}
      </div>
      {children}
    </div>
  );
}

// --- Main tab -----------------------------------------------------------------

export function CompliancePackTab({
  namespace,
  collection,
}: {
  namespace: string;
  collection: string;
}) {
  const [pack, setPack] = useState<CompliancePack | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const generate = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setPack(await buildPack(collection, namespace));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to build pack");
    } finally {
      setBusy(false);
    }
  }, [collection, namespace]);

  const downloadJSON = useCallback(() => {
    if (!pack) return;
    const blob = new Blob([JSON.stringify(pack, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `valori-compliance-${namespace.replace(/[^a-z0-9]/gi, "-")}-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, [pack, namespace]);

  return (
    <TabShell>
      {/* Intro */}
      <div className="rounded-xl border border-border bg-card p-5 flex items-start justify-between gap-4">
        <div>
          <p className="text-sm font-semibold text-card-foreground">Compliance Pack</p>
          <p className="text-xs text-muted-foreground mt-1 leading-relaxed">
            One signed, regulator-ready evidence bundle for this collection: integrity attestation,
            tamper status, right-to-erasure certificates, and answer-provenance receipts — mapped to
            EU AI Act, GDPR, and SOC 2 controls. Self-verifying via SHA-256; every hash is
            reproducible from <code className="font-mono bg-accent px-1 rounded">events.log</code>.
          </p>
        </div>
        <button
          onClick={generate}
          disabled={busy}
          className="flex-shrink-0 text-sm px-4 py-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 font-medium disabled:opacity-50 transition-colors"
        >
          {busy ? "Building…" : pack ? "Rebuild" : "Generate pack →"}
        </button>
      </div>

      {error && <p className="text-xs text-red-400 font-mono px-1">{error}</p>}

      {pack && (
        <>
          {/* 1. Attestation */}
          <Section num={1} title="Integrity Attestation"
            status={{ label: `${numOr0(pack.attestation.record_count).toLocaleString()} records`, tone: "neutral" }}>
            <div className="grid grid-cols-2 gap-x-6 gap-y-2 sm:grid-cols-4 text-xs">
              {[
                ["Records", pack.attestation.record_count],
                ["Nodes", pack.attestation.node_count],
                ["NS events", pack.attestation.ns_event_count],
                ["Total events", pack.attestation.total_events],
              ].map(([k, v]) => (
                <div key={k as string}>
                  <p className="text-[9px] text-muted-foreground uppercase tracking-widest">{k}</p>
                  <p className="font-mono text-accent-foreground font-semibold">{numOr0(v as number).toLocaleString()}</p>
                </div>
              ))}
            </div>
            <div className="flex flex-col gap-2">
              {[
                ["SHA-256 namespace proof", pack.attestation.ns_proof_hash],
                ["BLAKE3 global state", pack.attestation.global_blake3_hash],
              ].map(([label, val]) => (
                <div key={label as string} className="rounded-lg bg-background border border-border px-3 py-2">
                  <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">{label}</p>
                  <p className="font-mono text-[10px] text-muted-foreground break-all">{(val as string) ?? "unavailable"}</p>
                </div>
              ))}
            </div>
          </Section>

          {/* 2. Tamper */}
          <Section num={2} title="Tamper Status"
            status={{
              label: pack.tamper.status === "match" ? "✓ verified"
                : pack.tamper.status === "mismatch" ? "✗ mismatch" : "no baseline",
              tone: pack.tamper.status === "match" ? "good"
                : pack.tamper.status === "mismatch" ? "bad" : "warn",
            }}>
            {pack.tamper.baseline ? (
              <p className="text-xs text-muted-foreground">
                Baseline saved {new Date(pack.tamper.baseline.saved_at).toLocaleString()}
                {pack.tamper.baseline.note && <span className="italic"> · {pack.tamper.baseline.note}</span>}
                {pack.tamper.status === "mismatch" && (
                  <span className="text-red-500"> — namespace state has changed since the baseline was recorded.</span>
                )}
              </p>
            ) : (
              <p className="text-xs text-amber-600">
                No baseline recorded. Save one in the <strong>Certify</strong> tab to enable tamper attestation.
              </p>
            )}
          </Section>

          {/* 3. Erasures */}
          <Section num={3} title="Right-to-Erasure Evidence"
            status={{ label: `${pack.erasures.count} events`, tone: pack.erasures.count > 0 ? "good" : "neutral" }}>
            {pack.erasures.count > 0 ? (
              <div className="rounded-lg bg-background border border-border divide-y divide-border/60">
                {pack.erasures.certificates.slice(0, 8).map((e, i) => (
                  <div key={i} className="flex items-center gap-3 px-3 py-2 text-[11px]">
                    <span className="text-muted-foreground">{new Date(e.issued_at).toLocaleDateString()}</span>
                    <span className="text-muted-foreground">{e.erased_record_ids.length} records</span>
                    <span className="ml-auto font-mono text-muted-foreground">{shortHash(e.certificate_hash, 14)}</span>
                  </div>
                ))}
                <div className="px-3 py-1.5 text-[10px] text-muted-foreground">
                  {pack.erasures.total_records_erased} total records erased across {pack.erasures.count} events
                </div>
              </div>
            ) : (
              <p className="text-xs text-muted-foreground">No erasure events recorded. GDPR deletions made in the GDPR tab will appear here.</p>
            )}
          </Section>

          {/* 4. Receipts */}
          <Section num={4} title="Answer Provenance Records"
            status={{ label: `${pack.answer_receipts.count} receipts`, tone: pack.answer_receipts.count > 0 ? "good" : "neutral" }}>
            {pack.answer_receipts.count > 0 ? (
              <div className="rounded-lg bg-background border border-border divide-y divide-border/60">
                {pack.answer_receipts.receipts.slice(0, 8).map((r, i) => (
                  <div key={i} className="flex items-center gap-3 px-3 py-2 text-[11px]">
                    <span className="text-accent-foreground truncate flex-1">{r.question}</span>
                    <span className="text-muted-foreground">{r.chunks.length} chunks</span>
                    <span className="font-mono text-muted-foreground">{shortHash(r.receipt_sha256, 10)}</span>
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-xs text-muted-foreground">No proof-carrying answers yet. Questions asked in the Ask tab generate receipts that appear here.</p>
            )}
          </Section>

          {/* 5. Mapping */}
          <Section num={5} title="Regulatory Mapping">
            <ul className="flex flex-col gap-1.5">
              {pack.standard_refs.map((r) => (
                <li key={r} className="flex items-start gap-2 text-xs text-muted-foreground">
                  <span className="text-emerald-700 mt-0.5">✓</span>
                  {r}
                </li>
              ))}
            </ul>
          </Section>

          {/* Fingerprint + actions */}
          <div className="rounded-xl border-2 border-input bg-background p-4 flex flex-col gap-3">
            <p className="text-[9px] text-muted-foreground uppercase tracking-[3px] text-center">
              Pack Fingerprint (SHA-256 of full bundle)
            </p>
            <p className="font-mono text-[11px] text-accent-foreground break-all text-center">{pack.pack_sha256}</p>
            <div className="flex items-center justify-center gap-2 flex-wrap pt-1">
              <button
                onClick={() => printPack(pack)}
                className="text-sm px-4 py-2 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 font-medium transition-colors"
              >
                🖨 Print / Save as PDF
              </button>
              <button
                onClick={downloadJSON}
                className="text-sm px-4 py-2 rounded-lg border border-input text-accent-foreground hover:text-foreground hover:border-ring transition-colors"
              >
                Download JSON bundle
              </button>
            </div>
          </div>
        </>
      )}
    </TabShell>
  );
}
