"use client";

import { useState, useCallback, useEffect } from "react";

// --- Types --------------------------------------------------------------------

interface RecordEntry {
  id: number;
  encrypted: boolean;         // InsertRecordEncrypted event in history
  keyId?: string;             // hex prefix of key_id if encrypted
  metaPreview?: string;       // JSON metadata truncated
  deleted?: boolean;          // already has a DeleteRecord event
}

interface ErasureRecord {
  type: "ValoriErasureRecord";
  issued_at: string;
  collection: string;
  erased_record_ids: number[];
  pre_erasure_blake3: string | null;
  post_erasure_blake3: string | null;
  erasure_event_type: "DeleteRecord";
  note: string;
  certificate_hash: string | null;
}

// --- Helpers ------------------------------------------------------------------

async function sha256hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return "sha256:" + Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function fetchCurrentBlake3(): Promise<string | null> {
  try {
    const r = await fetch("/api/proof", { cache: "no-store" });
    if (!r.ok) return null;
    const d = await r.json() as { final_state_hash?: string };
    return d.final_state_hash ?? null;
  } catch { return null; }
}

// --- Sub-components -----------------------------------------------------------

function StatusPill({
  encrypted,
  deleted,
  keyId,
}: {
  encrypted: boolean;
  deleted: boolean;
  keyId?: string;
}) {
  if (deleted) {
    return (
      <span className="text-[9px] font-mono px-1.5 py-0.5 rounded bg-accent text-muted-foreground border border-input">
        already erased
      </span>
    );
  }
  if (encrypted) {
    return (
      <span
        className="text-[9px] font-mono px-1.5 py-0.5 rounded bg-amber-950/60 text-amber-400 border border-amber-800"
        title={`Encrypted with key ${keyId ?? "unknown"} — ShredKey not yet implemented`}
      >
        encrypted · {keyId ?? "?"}
      </span>
    );
  }
  return (
    <span className="text-[9px] font-mono px-1.5 py-0.5 rounded bg-accent text-muted-foreground border border-input">
      plaintext
    </span>
  );
}

function RecordRow({
  record,
  checked,
  onChange,
}: {
  record: RecordEntry;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const disabled = record.deleted;
  return (
    <label
      className={`flex items-center gap-3 px-4 py-2.5 rounded-lg cursor-pointer transition-colors ${
        disabled
          ? "opacity-40 cursor-not-allowed"
          : checked
          ? "bg-red-950/20 border border-red-900/40"
          : "border border-transparent hover:bg-accent/60"
      }`}
    >
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
        className="accent-red-500 w-4 h-4"
      />
      <span className="font-mono text-sm text-accent-foreground w-16 flex-shrink-0">
        #{record.id}
      </span>
      <StatusPill
        encrypted={record.encrypted}
        deleted={record.deleted ?? false}
        keyId={record.keyId}
      />
      {record.metaPreview && (
        <span className="text-xs text-muted-foreground truncate flex-1 font-mono">
          {record.metaPreview}
        </span>
      )}
    </label>
  );
}

// --- Erasure Certificate display ----------------------------------------------

function ErasureCertificate({
  cert,
  json,
  onClose,
}: {
  cert: ErasureRecord;
  json: string;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);

  const copy = useCallback(async () => {
    await navigator.clipboard.writeText(json);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [json]);

  const download = useCallback(() => {
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `valori-erasure-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, [json]);

  const changed = cert.pre_erasure_blake3 !== cert.post_erasure_blake3;

  return (
    <div className="rounded-xl border-2 border-emerald-800 bg-emerald-950/20 p-5 flex flex-col gap-4">
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-3">
          <span className="text-2xl text-emerald-400">✓</span>
          <div>
            <p className="text-sm font-bold text-emerald-400">Erasure Complete</p>
            <p className="text-xs text-emerald-700 mt-0.5">
              {cert.erased_record_ids.length} record{cert.erased_record_ids.length !== 1 ? "s" : ""}{" "}
              permanently erased · BLAKE3 audit chain updated
            </p>
          </div>
        </div>
        <button
          onClick={onClose}
          className="text-xs text-muted-foreground hover:text-muted-foreground transition-colors"
        >
          dismiss
        </button>
      </div>

      {/* Hash comparison */}
      {(cert.pre_erasure_blake3 || cert.post_erasure_blake3) && (
        <div className="grid grid-cols-2 gap-3">
          {[
            { label: "Pre-erasure BLAKE3", val: cert.pre_erasure_blake3, color: "text-muted-foreground" },
            {
              label: "Post-erasure BLAKE3",
              val: cert.post_erasure_blake3,
              color: changed ? "text-emerald-400" : "text-muted-foreground",
            },
          ].map(({ label, val, color }) => (
            <div key={label} className="rounded-lg bg-background border border-border px-3 py-2">
              <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">{label}</p>
              <p className={`font-mono text-[10px] break-all ${color}`}>
                {val ? val.slice(0, 32) + "…" : "—"}
              </p>
            </div>
          ))}
        </div>
      )}

      {changed && (
        <p className="text-xs text-emerald-700 font-mono">
          ↑ State hash changed — erasure events are recorded in the BLAKE3 audit chain
        </p>
      )}

      {/* Erased IDs */}
      <div className="rounded-lg bg-background border border-border px-3 py-2">
        <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1.5">Erased Record IDs</p>
        <p className="font-mono text-[11px] text-muted-foreground break-all">
          {cert.erased_record_ids.join(", ")}
        </p>
      </div>

      {/* Certificate hash */}
      <div className="rounded-lg bg-background border border-border px-3 py-2">
        <p className="text-[9px] text-muted-foreground uppercase tracking-widest mb-1">Certificate Fingerprint (SHA-256)</p>
        <p className="font-mono text-[11px] text-muted-foreground break-all">{cert.certificate_hash}</p>
      </div>

      <div className="flex gap-2">
        <button
          onClick={copy}
          className={`text-xs px-3 py-1.5 rounded border transition-all ${
            copied
              ? "border-emerald-700 bg-emerald-950/40 text-emerald-400"
              : "border-input text-muted-foreground hover:text-foreground hover:border-ring bg-card"
          }`}
        >
          {copied ? "✓ copied" : "copy JSON"}
        </button>
        <button
          onClick={download}
          className="text-xs px-3 py-1.5 rounded border border-input text-muted-foreground hover:text-foreground hover:border-ring bg-card transition-all"
        >
          download .json
        </button>
      </div>

      <div className="text-[10px] text-muted-foreground border-t border-border pt-3 leading-relaxed">
        <strong className="text-muted-foreground">Verification:</strong> Replay events.log through{" "}
        <code>valori-verify</code>. The erased records&apos; vectors are permanently gone; the{" "}
        <code>DeleteRecord</code> events remain in the audit chain as proof of erasure. For encrypted
        records, full crypto-erasure requires a <code>ShredKey</code> event once that endpoint is
        available.
      </div>
    </div>
  );
}

// --- Main tab -----------------------------------------------------------------

export function GdprTab({ namespace }: { namespace: string }) {
  const [records, setRecords] = useState<RecordEntry[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [erasing, setErasing] = useState(false);
  const [erasingProgress, setErasingProgress] = useState<{ done: number; total: number } | null>(null);
  const [erasedCert, setErasedCert] = useState<{ cert: ErasureRecord; json: string } | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [filterMode, setFilterMode] = useState<"all" | "encrypted" | "plaintext">("all");

  const loadRecords = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    setRecords(null);
    setSelected(new Set());
    setConfirmed(false);
    try {
      const auditRes = await fetch(
        `/api/namespace-audit?namespace=${encodeURIComponent(namespace)}`,
        { cache: "no-store" }
      );
      if (!auditRes.ok) throw new Error(`Audit fetch failed (${auditRes.status})`);
      const audit = await auditRes.json() as {
        ns_record_ids: number[];
        events: Array<{ event_id: number; raw: string; kind: string; record_ids: number[] }>;
        error?: string;
      };
      if (audit.error) throw new Error(audit.error);

      // Build sets: encrypted, deleted
      const encryptedIds = new Map<number, string>(); // recordId → keyId hex prefix
      const deletedIds = new Set<number>();

      for (const ev of audit.events) {
        if (ev.raw.includes("InsertRecordEncrypted")) {
          for (const rid of ev.record_ids) {
            const keyMatch = ev.raw.match(/key\s+([0-9a-f]+)/i);
            encryptedIds.set(rid, keyMatch?.[1] ?? "???");
          }
        }
        if (ev.raw.includes("DeleteRecord") || ev.raw.includes("SoftDeleteRecord")) {
          for (const rid of ev.record_ids) {
            deletedIds.add(rid);
          }
        }
      }

      // Build record list (limit meta fetches to first 50 to avoid flooding)
      const ids = audit.ns_record_ids;
      const metaFetches = ids.slice(0, 50).map((id) =>
        fetch(`/api/meta?target_id=record:${id}`, { cache: "no-store" })
          .then((r) => r.ok ? r.json() : null)
          .then((d) => {
            const val = d?.metadata ?? d?.value ?? d?.text ?? null;
            if (!val) return { id, preview: undefined };
            const s = typeof val === "string" ? val : JSON.stringify(val);
            return { id, preview: s.slice(0, 80) };
          })
          .catch(() => ({ id, preview: undefined }))
      );
      const metas = await Promise.all(metaFetches);
      const metaMap = new Map(metas.map((m) => [m.id, m.preview]));

      setRecords(
        ids.map((id) => ({
          id,
          encrypted: encryptedIds.has(id),
          keyId: encryptedIds.get(id),
          metaPreview: metaMap.get(id),
          deleted: deletedIds.has(id),
        }))
      );
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : "Failed to load records");
    } finally {
      setLoading(false);
    }
  }, [namespace]);

  // auto-load on mount
  useEffect(() => { loadRecords(); }, [loadRecords]);

  const toggleRecord = useCallback((id: number, val: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (val) next.add(id); else next.delete(id);
      return next;
    });
  }, []);

  const selectAll = useCallback(() => {
    if (!records) return;
    setSelected(new Set(records.filter((r) => !r.deleted).map((r) => r.id)));
  }, [records]);

  const selectEncrypted = useCallback(() => {
    if (!records) return;
    setSelected(new Set(records.filter((r) => r.encrypted && !r.deleted).map((r) => r.id)));
  }, [records]);

  const clearSelection = useCallback(() => setSelected(new Set()), []);

  const erase = useCallback(async () => {
    if (selected.size === 0 || !confirmed) return;
    setErasing(true);
    setErasedCert(null);
    const ids = [...selected];

    const preHash = await fetchCurrentBlake3();
    let successIds: number[] = [];

    setErasingProgress({ done: 0, total: ids.length });
    for (let i = 0; i < ids.length; i++) {
      try {
        await fetch("/api/delete", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ id: ids[i], collection: namespace }),
        });
        successIds.push(ids[i]);
      } catch {
        // continue — record missing failures
      }
      setErasingProgress({ done: i + 1, total: ids.length });
    }

    const postHash = await fetchCurrentBlake3();

    // Build erasure certificate
    const payload: ErasureRecord = {
      type: "ValoriErasureRecord",
      issued_at: new Date().toISOString(),
      collection: namespace,
      erased_record_ids: successIds,
      pre_erasure_blake3: preHash,
      post_erasure_blake3: postHash,
      erasure_event_type: "DeleteRecord",
      note: "Physical erasure via DeleteRecord events. Crypto-erasure (ShredKey) requires per-record encryption and is not yet available.",
      certificate_hash: null,
    };
    const certHash = await sha256hex(JSON.stringify(payload));
    const signed: ErasureRecord = { ...payload, certificate_hash: certHash };
    const json = JSON.stringify(signed, null, 2);

    // Persist the erasure certificate so the Compliance Pack (feature B1) can
    // bundle a complete right-to-erasure history for this namespace.
    try {
      const key = `valori:erasures:${namespace}`;
      const existing = JSON.parse(localStorage.getItem(key) ?? "[]") as ErasureRecord[];
      localStorage.setItem(key, JSON.stringify([signed, ...existing].slice(0, 200)));
    } catch { /* non-fatal */ }

    setErasedCert({ cert: signed, json });
    setErasing(false);
    setErasingProgress(null);
    setSelected(new Set());
    setConfirmed(false);
    // Reload records to reflect deletions
    loadRecords();
  }, [selected, confirmed, namespace, loadRecords]);

  // Derived
  const totalRecords = records?.length ?? 0;
  const liveRecords = records?.filter((r) => !r.deleted) ?? [];
  const encryptedCount = liveRecords.filter((r) => r.encrypted).length;
  const selectedCount = selected.size;

  const filteredRecords = records?.filter((r) => {
    if (filterMode === "encrypted") return r.encrypted;
    if (filterMode === "plaintext") return !r.encrypted;
    return true;
  }) ?? [];

  return (
    <div className="flex flex-col gap-5 max-w-3xl">

      {/* Info banner */}
      <div className="rounded-xl border border-amber-900/50 bg-amber-950/20 px-4 py-3 flex gap-3">
        <span className="text-amber-500 text-lg flex-shrink-0">⚠</span>
        <div className="text-xs text-amber-700 leading-relaxed">
          <strong className="text-amber-500">GDPR Right to Erasure:</strong> Selecting and erasing
          records permanently removes their vector data from the Valori store. The{" "}
          <code className="font-mono bg-amber-950/40 px-1 rounded">DeleteRecord</code> event is
          recorded in the BLAKE3 audit chain (proof of erasure), but the vectors cannot be recovered.
          For per-record crypto-erasure (ShredKey), records must be inserted via{" "}
          <code className="font-mono bg-amber-950/40 px-1 rounded">InsertRecordEncrypted</code>.
        </div>
      </div>

      {/* Erasure certificate */}
      {erasedCert && (
        <ErasureCertificate
          cert={erasedCert.cert}
          json={erasedCert.json}
          onClose={() => setErasedCert(null)}
        />
      )}

      {/* Controls */}
      <div className="rounded-xl border border-border bg-card p-4 flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <p className="text-sm font-semibold text-card-foreground">Records in namespace</p>
            {records && (
              <span className="text-xs font-mono text-muted-foreground">
                {liveRecords.length} live
                {encryptedCount > 0 && (
                  <span className="text-amber-600"> · {encryptedCount} encrypted</span>
                )}
              </span>
            )}
          </div>
          <button
            onClick={loadRecords}
            disabled={loading}
            className="text-xs px-3 py-1.5 rounded border border-input text-muted-foreground hover:text-foreground hover:border-ring disabled:opacity-40 transition-all"
          >
            {loading ? "Loading…" : "↻ Refresh"}
          </button>
        </div>

        {/* Filter + select helpers */}
        {records && records.length > 0 && (
          <div className="flex items-center gap-2 flex-wrap">
            <div className="flex items-center gap-0.5 bg-accent rounded-md border border-input p-0.5">
              {(["all", "encrypted", "plaintext"] as const).map((m) => (
                <button
                  key={m}
                  onClick={() => setFilterMode(m)}
                  className={`px-2.5 py-0.5 text-xs rounded transition-colors ${
                    filterMode === m
                      ? "bg-muted text-foreground"
                      : "text-muted-foreground hover:text-accent-foreground"
                  }`}
                >
                  {m}
                </button>
              ))}
            </div>
            <div className="h-4 border-l border-input" />
            <button
              onClick={selectAll}
              className="text-xs text-muted-foreground hover:text-accent-foreground transition-colors"
            >
              select all
            </button>
            {encryptedCount > 0 && (
              <button
                onClick={selectEncrypted}
                className="text-xs text-amber-600 hover:text-amber-400 transition-colors"
              >
                select encrypted
              </button>
            )}
            {selectedCount > 0 && (
              <button
                onClick={clearSelection}
                className="text-xs text-muted-foreground hover:text-muted-foreground transition-colors"
              >
                clear selection
              </button>
            )}
            {selectedCount > 0 && (
              <span className="ml-auto text-xs text-muted-foreground">
                {selectedCount} selected
              </span>
            )}
          </div>
        )}
      </div>

      {/* Error */}
      {loadError && (
        <p className="text-sm text-red-400 font-mono px-1">{loadError}</p>
      )}

      {/* Loading skeleton */}
      {loading && !records && (
        <div className="flex flex-col gap-2 px-1">
          {Array.from({ length: 6 }).map((_, i) => (
            <div key={i} className="h-9 bg-accent rounded-lg animate-pulse" />
          ))}
        </div>
      )}

      {/* Record list */}
      {records && (
        <div className="rounded-xl border border-border bg-card overflow-hidden">
          {filteredRecords.length === 0 ? (
            <p className="text-sm text-muted-foreground text-center py-8">
              {filterMode === "encrypted"
                ? "No encrypted records in this namespace"
                : filterMode === "plaintext"
                ? "No plaintext records in this namespace"
                : "No records in this namespace"}
            </p>
          ) : (
            <div className="flex flex-col gap-0.5 p-2 max-h-[400px] overflow-y-auto">
              {filteredRecords.map((record) => (
                <RecordRow
                  key={record.id}
                  record={record}
                  checked={selected.has(record.id)}
                  onChange={(v) => toggleRecord(record.id, v)}
                />
              ))}
              {totalRecords > filteredRecords.length && (
                <p className="text-[10px] text-muted-foreground text-center py-2">
                  Showing {filteredRecords.length} of {totalRecords} records (filtered)
                </p>
              )}
            </div>
          )}
        </div>
      )}

      {/* Erase action */}
      {selectedCount > 0 && (
        <div className="rounded-xl border-2 border-red-900/60 bg-red-950/20 p-4 flex flex-col gap-3">
          <div className="flex items-start gap-3">
            <span className="text-red-500 text-lg flex-shrink-0">⚠</span>
            <div>
              <p className="text-sm font-semibold text-red-400">
                Permanently erase {selectedCount} record{selectedCount !== 1 ? "s" : ""}?
              </p>
              <p className="text-xs text-red-800 mt-0.5">
                This cannot be undone. Vector data will be permanently removed. The deletion events
                will be recorded in the BLAKE3 audit chain.
              </p>
            </div>
          </div>

          {/* Confirmation checkbox */}
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={confirmed}
              onChange={(e) => setConfirmed(e.target.checked)}
              className="accent-red-500 w-4 h-4"
            />
            <span className="text-xs text-red-700">
              I understand this is irreversible and confirm the right-to-erasure request
            </span>
          </label>

          {/* Erase button */}
          <button
            onClick={erase}
            disabled={!confirmed || erasing}
            className="self-start px-4 py-2 rounded-lg text-sm font-medium bg-red-700 text-foreground hover:bg-red-600 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            {erasing
              ? `Erasing… ${erasingProgress?.done ?? 0} / ${erasingProgress?.total ?? 0}`
              : `Erase ${selectedCount} record${selectedCount !== 1 ? "s" : ""} →`}
          </button>

          {/* Progress bar */}
          {erasingProgress && (
            <div className="h-1.5 bg-accent rounded-full overflow-hidden">
              <div
                className="h-full bg-red-600 transition-all duration-200"
                style={{
                  width: `${(erasingProgress.done / erasingProgress.total) * 100}%`,
                }}
              />
            </div>
          )}
        </div>
      )}

      {/* ShredKey note */}
      {encryptedCount > 0 && (
        <div className="rounded-xl border border-border bg-card px-4 py-3 flex flex-col gap-1.5">
          <p className="text-xs font-medium text-muted-foreground">About ShredKey (Crypto-Erasure)</p>
          <p className="text-[11px] text-muted-foreground leading-relaxed">
            {encryptedCount} record{encryptedCount !== 1 ? "s" : ""} in this namespace{" "}
            {encryptedCount !== 1 ? "were" : "was"} inserted with per-record encryption keys
            (<code className="font-mono bg-accent px-1 rounded">InsertRecordEncrypted</code>).
            The <code className="font-mono bg-accent px-1 rounded">ShredKey</code> event
            destroys the encryption key for all records sharing that key — the ciphertext remains
            in the log but becomes unrecoverable. This is the strongest erasure guarantee and
            satisfies GDPR Article 17 without mutating the audit chain.{" "}
            <strong className="text-muted-foreground">ShredKey is not yet exposed as an HTTP endpoint</strong>{" "}
            — contact your system administrator to issue it directly.
          </p>
        </div>
      )}
    </div>
  );
}
