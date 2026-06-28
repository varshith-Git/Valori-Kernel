"use client";

import { use, useState, useCallback, useRef, useEffect } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { MultiSearch } from "@/components/collections/MultiSearch";
import { DocumentUploadTab } from "@/components/ingestion/DocumentUploadTab";
import { GraphTab } from "@/components/collections/GraphTab";
import { AskTab } from "@/components/collections/AskTab";
import { DocumentsTab } from "@/components/collections/DocumentsTab";
import { VerifyTab } from "@/components/collections/VerifyTab";
import { EvalTab } from "@/components/collections/EvalTab";
import { CertifyTab } from "@/components/collections/CertifyTab";
import { GdprTab } from "@/components/collections/GdprTab";
import { DiffTab } from "@/components/collections/DiffTab";
import { ContradictionTab } from "@/components/collections/ContradictionTab";
import { CompliancePackTab } from "@/components/collections/CompliancePackTab";
import { CommunityTab } from "@/components/collections/CommunityTab";
import { EntityExtractionTab } from "@/components/collections/EntityExtractionTab";
import { useHealth } from "@/lib/hooks/useHealth";
import { makeNs } from "@/lib/hooks/useCollections";
import { cn } from "@/lib/utils";

/* -- Tab registry ---------------------------------------------------- */

/** Primary tabs shown directly in the tab bar */
const PRIMARY_TABS = [
  { value: "search",  label: "Search",    tip: "Find records by semantic similarity, ID, or regex" },
  { value: "upload",  label: "Upload",    tip: "Ingest PDF / DOCX / TXT with auto-chunking and embedding" },
  { value: "ask",     label: "Ask",       tip: "Natural-language Q&A with LLM synthesis over top-K chunks" },
  { value: "docs",    label: "Documents", tip: "Browse ingested documents and their chunks" },
];

/** Power-user tabs hidden behind the overflow menu */
const OVERFLOW_TABS = [
  { value: "community",  label: "Communities",   tip: "Label Propagation community detection + centroid search — find themes across the entire graph" },
  { value: "entities",   label: "Entity Extract",tip: "LLM extracts named entities + relationships from text and inserts them as graph nodes + edges" },
  { value: "graph",      label: "Graph",         tip: "Visualise Document→Chunk relationships and entity links" },
  { value: "verify",     label: "Verify",        tip: "Compute SHA-256 namespace proof hash — reproducible from events.log" },
  { value: "eval",       label: "Eval",          tip: "Score retrieval quality with ground-truth QA pairs: Precision@K, MRR" },
  { value: "certify",    label: "Certify",       tip: "Signed JSON + PDF proof certificate with tamper detection" },
  { value: "gdpr",       label: "GDPR",          tip: "Right-to-erasure with BLAKE3-chained erasure certificate" },
  { value: "diff",       label: "Diff",          tip: "Compare two namespaces by record/node ID set difference" },
  { value: "contradict", label: "Contradictions",tip: "Find semantically opposing chunks by negating embeddings" },
  { value: "compliance", label: "Compliance",    tip: "Regulator evidence bundle (EU AI Act / GDPR / SOC 2)" },
  { value: "info",       label: "Info",          tip: "Collection metadata: namespace ID, vector dimension, storage details" },
];

const ALL_TABS = [...PRIMARY_TABS, ...OVERFLOW_TABS];

/* -- Overflow dropdown ----------------------------------------------- */

function OverflowMenu({
  activeValue,
  onSelect,
}: {
  activeValue: string;
  onSelect: (v: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const activeOverflow = OVERFLOW_TABS.find((t) => t.value === activeValue);

  useEffect(() => {
    function onClickOutside(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onClickOutside);
    return () => document.removeEventListener("mousedown", onClickOutside);
  }, []);

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-all",
          activeOverflow
            ? "bg-muted text-foreground"
            : "text-muted-foreground hover:bg-accent hover:text-card-foreground"
        )}
      >
        {activeOverflow ? activeOverflow.label : "Tools"}
        <ChevronDown size={13} className={cn("transition-transform", open && "rotate-180")} />
      </button>

      {open && (
        <div className="absolute left-0 top-full z-50 mt-1.5 w-52 rounded-xl border border-input bg-card shadow-xl shadow-black/40 py-1 overflow-hidden">
          {OVERFLOW_TABS.map((t) => (
            <button
              key={t.value}
              title={t.tip}
              onClick={() => { onSelect(t.value); setOpen(false); }}
              className={cn(
                "w-full flex items-center gap-2 px-3 py-2 text-sm text-left transition-colors",
                activeValue === t.value
                  ? "bg-[var(--v-accent-muted)] text-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-card-foreground"
              )}
            >
              <ChevronRight size={12} className="shrink-0 text-muted-foreground" />
              <span>{t.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/* -- Page ------------------------------------------------------------ */

export default function CollectionPage({
  params,
}: {
  params: Promise<{ name: string; collection: string }>;
}) {
  const { name, collection } = use(params);
  const project = decodeURIComponent(name);
  const col = decodeURIComponent(collection);
  const namespace = makeNs(project, col);

  const { dim } = useHealth();
  const [activeTab, setActiveTab] = useState("search");
  const [pendingQuestion, setPendingQuestion] = useState("");

  const handleAskQuestion = useCallback((q: string) => {
    setPendingQuestion(q);
    setActiveTab("ask");
  }, []);

  const deleteRecord = async (id: number) => {
    await fetch("/api/delete", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id }),
    });
  };

  const isOverflow = OVERFLOW_TABS.some((t) => t.value === activeTab);

  return (
    <div className="flex flex-col gap-5 max-w-4xl">
      {/* Tab bar: primary + overflow */}
      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <div className="flex items-center gap-1 flex-wrap">
          <code className="text-[10px] text-muted-foreground font-mono bg-card px-2 py-0.5 rounded border border-border self-center">
            {namespace}
          </code>
          <TabsList className="h-auto bg-card border border-border p-1 gap-0.5 flex-wrap">
            {PRIMARY_TABS.map(({ value, label, tip }) => (
              <TabsTrigger
                key={value}
                value={value}
                title={tip}
                className="data-[state=active]:bg-[var(--v-accent-muted)] data-[state=active]:text-foreground data-[state=active]:[box-shadow:inset_0_-2px_0_var(--v-accent)] text-muted-foreground transition-all"
              >
                {label}
              </TabsTrigger>
            ))}
          </TabsList>

          {/* Overflow dropdown lives beside the TabsList (not inside it so it doesn't break Radix) */}
          <div className={cn(
            "h-9 flex items-center rounded-md border px-0.5",
            isOverflow ? "border-input bg-card" : "border-transparent bg-transparent"
          )}>
            <OverflowMenu activeValue={activeTab} onSelect={setActiveTab} />
          </div>
        </div>

        {/* Tab content for all panels */}
        <TabsContent value="search" className="mt-5">
          <MultiSearch namespace={namespace} dim={dim} onDelete={deleteRecord} />
        </TabsContent>
        <TabsContent value="upload" className="mt-5">
          <DocumentUploadTab collection={namespace} onAskQuestion={handleAskQuestion} />
        </TabsContent>
        <TabsContent value="ask" className="mt-5">
          <AskTab namespace={namespace} initialQuestion={pendingQuestion} />
        </TabsContent>
        <TabsContent value="docs" className="mt-5">
          <DocumentsTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="community" className="mt-5">
          <CommunityTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="entities" className="mt-5">
          <EntityExtractionTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="graph" className="mt-5">
          <GraphTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="verify" className="mt-5">
          <VerifyTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="eval" className="mt-5">
          <EvalTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="certify" className="mt-5">
          <CertifyTab namespace={namespace} collection={col} />
        </TabsContent>
        <TabsContent value="gdpr" className="mt-5">
          <GdprTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="diff" className="mt-5">
          <DiffTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="contradict" className="mt-5">
          <ContradictionTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="compliance" className="mt-5">
          <CompliancePackTab namespace={namespace} collection={col} />
        </TabsContent>
        <TabsContent value="info" className="mt-5">
          <CollectionInfo project={project} collection={col} namespace={namespace} dim={dim} />
        </TabsContent>
      </Tabs>
    </div>
  );
}

/* -- Collection info panel ------------------------------------------- */

function CollectionInfo({
  project,
  collection,
  namespace,
  dim,
}: {
  project: string;
  collection: string;
  namespace: string;
  dim: number | null;
}) {
  return (
    <div className="flex flex-col gap-4 max-w-md">
      <div className="rounded-xl border border-border bg-card divide-y divide-border">
        <InfoRow label="Project" value={project} />
        <InfoRow label="Collection" value={collection} />
        <InfoRow label="Namespace (Valori)" value={namespace} mono />
        <InfoRow label="Dimension" value={dim != null ? String(dim) : "—"} />
        <InfoRow
          label="Storage per record"
          value={dim != null ? `${dim * 4} bytes` : "—"}
          sub={dim != null ? `${dim} scalars × 4 B (Q16.16)` : undefined}
        />
        <InfoRow label="Search modes" value="Semantic · #id · Regex" />
        <InfoRow label="Pending modes" value="Text · Hybrid · Metadata" sub="requires embedding API" />
      </div>
    </div>
  );
}

function InfoRow({
  label,
  value,
  sub,
  mono,
}: {
  label: string;
  value: string;
  sub?: string;
  mono?: boolean;
}) {
  return (
    <div className="flex items-start justify-between px-4 py-3">
      <p className="text-sm text-muted-foreground">{label}</p>
      <div className="text-right">
        <span className={`text-sm ${mono ? "font-mono" : ""} text-accent-foreground`}>{value}</span>
        {sub && <p className="text-xs text-muted-foreground mt-0.5">{sub}</p>}
      </div>
    </div>
  );
}
