"use client"

import { useState } from "react"
import { TabShell } from "@/components/collections/TabShell"
import { useTransport } from "@/runtime/context"

type Claim = { subject: string; predicate: string; object: string; negated: boolean; time_scope: string }
type Receipt = { verification_id: string; outcome: "Supports" | "Contradicts" | "Neutral" | "Unknown"; receipt_hash: string; input_assertion_ids: string[]; confidence?: string }

const emptyClaim = (): Claim => ({ subject: "", predicate: "", object: "", negated: false, time_scope: "" })

export function AssertionsTab({ projectId }: { projectId: string; namespace: string }) {
  const transport = useTransport()
  const [left, setLeft] = useState<Claim>(() => emptyClaim())
  const [right, setRight] = useState<Claim>(() => emptyClaim())
  const [leftId, setLeftId] = useState("assertion-left")
  const [rightId, setRightId] = useState("assertion-right")
  const [receipt, setReceipt] = useState<Receipt | null>(null)
  const [lookup, setLookup] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function verify() {
    setBusy(true); setError(null)
    try {
      const res = await fetch(transport.path(projectId, "/assertions/verify"), { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ left, right, left_assertion_id: leftId, right_assertion_id: rightId }) })
      const body = await res.json()
      if (!res.ok) throw new Error(body.error ?? `Verification failed (${res.status})`)
      setReceipt(body as Receipt)
    } catch (e) { setError(e instanceof Error ? e.message : String(e)) } finally { setBusy(false) }
  }

  async function load() {
    setBusy(true); setError(null)
    try {
      const res = await fetch(transport.path(projectId, `/assertions/verification/${encodeURIComponent(lookup)}`))
      if (!res.ok) throw new Error(`Receipt lookup failed (${res.status})`)
      setReceipt((await res.json()) as Receipt)
    } catch (e) { setError(e instanceof Error ? e.message : String(e)) } finally { setBusy(false) }
  }

  const editor = (title: string, claim: Claim, setClaim: (c: Claim) => void, id: string, setId: (v: string) => void) => (
    <div className="rounded-xl border bg-card p-4 space-y-3">
      <div className="font-medium">{title}</div>
      <input className="w-full rounded-md border bg-background px-3 py-2 text-sm" placeholder="Assertion ID" value={id} onChange={e => setId(e.target.value)} />
      {(["subject", "predicate", "object", "time_scope"] as const).map(key => <input key={key} className="w-full rounded-md border bg-background px-3 py-2 text-sm" placeholder={key.replace("_", " ")} value={claim[key]} onChange={e => setClaim({ ...claim, [key]: e.target.value })} />)}
      <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={claim.negated} onChange={e => setClaim({ ...claim, negated: e.target.checked })} /> Explicitly negated</label>
    </div>
  )

  return <TabShell>
    <div><h2 className="text-lg font-semibold">Claim verification</h2><p className="text-sm text-muted-foreground">Compare two evidence-backed assertions. Similarity and citation alone never create a semantic result.</p></div>
    <div className="grid gap-4 md:grid-cols-2">{editor("Assertion A", left, setLeft, leftId, setLeftId)}{editor("Assertion B", right, setRight, rightId, setRightId)}</div>
    <button className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50" disabled={busy} onClick={verify}>{busy ? "Verifying…" : "Verify claims"}</button>
    <div className="flex gap-2"><input className="flex-1 rounded-md border bg-background px-3 py-2 text-sm" placeholder="Verification ID" value={lookup} onChange={e => setLookup(e.target.value)} /><button className="rounded-md border px-4 py-2 text-sm" disabled={!lookup || busy} onClick={load}>Load receipt</button></div>
    {error && <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">{error}</div>}
    {receipt && <div className="rounded-xl border bg-card p-4 space-y-2"><div className="flex items-center justify-between"><span className="font-medium">Result</span><span className="rounded-full border px-3 py-1 text-sm font-semibold">{receipt.outcome}</span></div><div className="text-xs text-muted-foreground break-all">Verification ID: {receipt.verification_id}</div><div className="text-xs text-muted-foreground break-all">Receipt hash: {receipt.receipt_hash}</div></div>}
  </TabShell>
}
