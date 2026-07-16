"use client";

import { use, useState, useCallback, useRef, useEffect } from "react";
import { ChevronDown, ChevronRight, Users, Wrench, Terminal, Database, Layers, BookOpen, SlidersHorizontal } from "lucide-react";
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
import { TreeRagTab } from "@/components/collections/TreeRagTab";
import { BulkInsertTab } from "@/components/collections/BulkInsertTab";
import { VisualizeTab } from "@/components/collections/VisualizeTab";
import { TabShell } from "@/components/collections/TabShell";
import { useHealth } from "@/lib/hooks/useHealth";
import { makeNs } from "@/lib/hooks/useCollections";
import { cn } from "@/lib/utils";

/* -- Tab registry ---------------------------------------------------- */

/** Primary tabs shown directly in the tab bar */
const PRIMARY_TABS = [
  { value: "search",    label: "Search",     tip: "Find records by semantic similarity, ID, or regex" },
  { value: "upload",    label: "Upload",     tip: "Ingest PDF / DOCX / TXT with auto-chunking and embedding" },
  { value: "bulk",      label: "Bulk Insert",tip: "Insert multiple vectors at once from CSV or JSON" },
  { value: "visualize", label: "Visualize",  tip: "2D PCA scatter plot of all vectors in this collection" },
  { value: "ask",       label: "Ask",        tip: "Natural-language Q&A with LLM synthesis over top-K chunks" },
  { value: "docs",      label: "Documents",  tip: "Browse ingested documents and their chunks" },
];

/** Analyze tabs — graph, entities, evaluation */
const ANALYZE_TABS = [
  { value: "treerag",    label: "Tree-RAG",      tip: "Navigate a document's section tree by term frequency — line-cited answers + BLAKE3 receipt chain" },
  { value: "community",  label: "Communities",   tip: "Label Propagation community detection + centroid search — find themes across the entire graph" },
  { value: "entities",   label: "Entity Extract",tip: "LLM extracts named entities + relationships from text and inserts them as graph nodes + edges" },
  { value: "graph",      label: "Graph",         tip: "Visualise Document→Chunk relationships and entity links" },
  { value: "eval",       label: "Eval",          tip: "Score retrieval quality with ground-truth QA pairs: Precision@K, MRR" },
  { value: "diff",       label: "Diff",          tip: "Compare two namespaces by record/node ID set difference" },
  { value: "contradict", label: "Contradictions",tip: "Find semantically opposing chunks by negating embeddings" },
  { value: "info",       label: "Info",          tip: "Collection metadata: namespace ID, vector dimension, storage details" },
];

/** Compliance tabs — proof, audit, certification */
const COMPLIANCE_TABS = [
  { value: "verify",     label: "Verify",        tip: "Compute SHA-256 namespace proof hash — reproducible from events.log" },
  { value: "certify",    label: "Certify",       tip: "Signed JSON + PDF proof certificate with tamper detection" },
  { value: "gdpr",       label: "GDPR",          tip: "Right-to-erasure with BLAKE3-chained erasure certificate" },
  { value: "compliance", label: "Compliance",    tip: "Regulator evidence bundle (EU AI Act / GDPR / SOC 2)" },
];

const OVERFLOW_TABS = [...ANALYZE_TABS, ...COMPLIANCE_TABS];

const ALL_TABS = [...PRIMARY_TABS, ...OVERFLOW_TABS];

/* -- Group overflow dropdown ----------------------------------------- */

type TabDef = { value: string; label: string; tip: string };

function GroupMenu({
  label,
  tabs,
  activeValue,
  onSelect,
}: {
  label: string;
  tabs: TabDef[];
  activeValue: string;
  onSelect: (v: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const activeTab = tabs.find((t) => t.value === activeValue);

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
          activeTab
            ? "bg-[var(--v-accent-muted)] text-foreground"
            : "text-muted-foreground hover:bg-accent hover:text-card-foreground"
        )}
      >
        {activeTab ? activeTab.label : label}
        <ChevronDown size={13} className={cn("transition-transform", open && "rotate-180")} />
      </button>

      {open && (
        <div className="absolute left-0 top-full z-50 mt-1.5 w-52 rounded-xl border border-input bg-card shadow-xl shadow-black/40 py-1 overflow-hidden">
          {tabs.map((t) => (
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

  const { dim, online, index } = useHealth();
  const [activeTab, setActiveTab] = useState("search");
  const [pendingQuestion, setPendingQuestion] = useState("");

  const handleAskQuestion = useCallback((q: string) => {
    setPendingQuestion(q);
    setActiveTab("ask");
  }, []);

  const deleteRecord = async (id: number) => {
    if (!window.confirm(`Delete record #${id}? This cannot be undone.`)) return;
    const res = await fetch("/api/delete", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id }),
    });
    if (!res.ok) {
      const body = await res.json().catch(() => ({})) as { error?: string };
      throw new Error(body.error ?? `Delete failed (${res.status})`);
    }
  };

  const isAnalyze = ANALYZE_TABS.some((t) => t.value === activeTab);
  const isCompliance = COMPLIANCE_TABS.some((t) => t.value === activeTab);

  return (
    <div className="flex flex-col gap-5 w-full max-w-[1600px]">
      {/* Collection header */}
      <CollectionHeader
        project={project}
        collection={col}
        namespace={namespace}
        dim={dim}
        online={online}
        index={index}
        onViewDetails={() => setActiveTab("info")}
      />

      {/* Tab bar: primary + two named group menus */}
      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <div className="flex items-center gap-1 flex-wrap border-b border-border">
          <TabsList className="h-auto bg-transparent border-0 p-0 gap-0 flex-wrap">
            {PRIMARY_TABS.map(({ value, label, tip }) => (
              <TabsTrigger
                key={value}
                value={value}
                title={tip}
                className="rounded-none border-b-2 border-transparent data-[state=active]:border-[var(--v-accent)] data-[state=active]:text-foreground text-muted-foreground bg-transparent px-4 py-2.5 text-sm font-medium hover:text-foreground transition-colors"
              >
                {label}
              </TabsTrigger>
            ))}
          </TabsList>

          {/* Analyze group */}
          <div className={cn(
            "flex items-center border-b-2 pb-[1px]",
            isAnalyze ? "border-[var(--v-accent)]" : "border-transparent"
          )}>
            <GroupMenu label="Analyze" tabs={ANALYZE_TABS} activeValue={activeTab} onSelect={setActiveTab} />
          </div>

          {/* Compliance group */}
          <div className={cn(
            "flex items-center border-b-2 pb-[1px]",
            isCompliance ? "border-[var(--v-accent)]" : "border-transparent"
          )}>
            <GroupMenu label="Compliance" tabs={COMPLIANCE_TABS} activeValue={activeTab} onSelect={setActiveTab} />
          </div>
        </div>

        {/* Tab content for all panels */}
        <TabsContent value="search" className="mt-5">
          <MultiSearch namespace={namespace} dim={dim} onDelete={deleteRecord} />
        </TabsContent>
        <TabsContent value="upload" className="mt-5">
          <DocumentUploadTab collection={namespace} onAskQuestion={handleAskQuestion} />
        </TabsContent>
        <TabsContent value="bulk" className="mt-5">
          <BulkInsertTab namespace={namespace} dim={dim} />
        </TabsContent>
        <TabsContent value="visualize" className="mt-5">
          <VisualizeTab namespace={namespace} dim={dim} />
        </TabsContent>
        <TabsContent value="ask" className="mt-5">
          <AskTab namespace={namespace} initialQuestion={pendingQuestion} />
        </TabsContent>
        <TabsContent value="docs" className="mt-5">
          <DocumentsTab namespace={namespace} />
        </TabsContent>
        <TabsContent value="treerag" className="mt-5">
          <TreeRagTab namespace={namespace} />
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

/* -- Collection header ------------------------------------------------ */

const ICON_VARIANTS = [
  { Icon: Users,    bg: "bg-blue-500/10",    color: "text-blue-500" },
  { Icon: Wrench,   bg: "bg-rose-500/10",    color: "text-rose-500" },
  { Icon: Terminal, bg: "bg-emerald-500/10", color: "text-emerald-600 dark:text-emerald-400" },
  { Icon: Database, bg: "bg-purple-500/10",  color: "text-purple-500" },
  { Icon: Layers,   bg: "bg-amber-500/10",   color: "text-amber-500" },
  { Icon: BookOpen, bg: "bg-cyan-500/10",    color: "text-cyan-500" },
];

function getIconVariant(name: string) {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xffff;
  return ICON_VARIANTS[h % ICON_VARIANTS.length];
}

function CollectionHeader({
  project,
  collection,
  namespace,
  dim,
  online,
  index: indexKind,
  onViewDetails,
}: {
  project: string;
  collection: string;
  namespace: string;
  dim: number | null;
  online: boolean;
  index: string | null;
  onViewDetails: () => void;
}) {
  const { Icon, bg, color } = getIconVariant(collection);
  const stats = [
    { label: "Vectors",   value: "—" },
    { label: "Records",   value: "—" },
    { label: "Dimension", value: dim != null ? String(dim) : "—" },
    { label: "Index",     value: indexKind ?? "—" },
    { label: "Shards",    value: "1" },
    { label: "Updated",   value: "—" },
    {
      label: "Status",
      value: online ? "Healthy" : "Unreachable",
      className: online
        ? "text-emerald-600 dark:text-emerald-400"
        : "text-amber-600 dark:text-amber-400",
      dot: online ? "bg-emerald-500" : "bg-amber-500",
    },
  ];

  return (
    <div className="rounded-xl border border-border bg-card px-5 py-4 flex items-center gap-4">
      <div className={cn("w-11 h-11 rounded-xl flex items-center justify-center shrink-0", bg)}>
        <Icon size={20} className={color} />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2.5 mb-2">
          <h1 className="text-lg font-semibold text-foreground">{collection}</h1>
          <span className="text-xs font-medium bg-[var(--v-accent-muted)] text-[var(--v-accent)] border border-[var(--v-accent)]/20 rounded-full px-2 py-0.5">
            Collection
          </span>
        </div>
        <div className="flex items-center gap-4 flex-wrap">
          {stats.map(({ label, value, className, dot }) => (
            <div key={label} className="flex items-center gap-1.5">
              <span className="text-[11px] text-muted-foreground">{label}</span>
              {dot && <span className={cn("w-1.5 h-1.5 rounded-full shrink-0", dot)} />}
              <span className={cn("text-xs font-semibold text-foreground", className)}>{value}</span>
            </div>
          ))}
        </div>
      </div>
      <button
        onClick={onViewDetails}
        className="shrink-0 text-xs font-medium border border-border rounded-lg px-3 py-1.5 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors flex items-center gap-1.5"
      >
        <SlidersHorizontal size={12} /> View details
      </button>
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
    <TabShell>
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
    </TabShell>
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
