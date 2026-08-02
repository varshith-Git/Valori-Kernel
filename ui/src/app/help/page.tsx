"use client";

import Link from "next/link";
import { useState, useRef, useEffect, useCallback } from "react";

// ── Data ─────────────────────────────────────────────────────────────────────

type Category = {
  id: string;
  icon: string;
  title: string;
  stripe: string;          // left border + dot color (Tailwind arbitrary)
  badge: string;           // pill badge
  header: string;          // section header text
  items: Item[];
};

type Item = {
  label: string;
  when: string;
  why: string;
  where: string;
};

const CATEGORIES: Category[] = [
  {
    id: "find",
    icon: "⊙",
    title: "Find information",
    stripe: "border-blue-500",
    badge: "bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20",
    header: "text-blue-600 dark:text-blue-400",
    items: [
      {
        label: "Semantic search",
        when: "You have a natural-language question or concept (e.g. 'treatment side effects').",
        why: "Converts your text to a vector using the configured embedding model, then finds the nearest chunks in the collection.",
        where: "Collection → Search → Text query",
      },
      {
        label: "Raw vector search",
        when: "You already have a float vector (e.g. from your application code).",
        why: "Bypasses embedding — sends the vector directly to Valori's HNSW/brute-force index.",
        where: "Collection → Search → Raw vector",
      },
      {
        label: "Record #id lookup",
        when: "You know the exact record ID you want to inspect.",
        why: "Direct lookup — no vector search, instant result.",
        where: "Collection → Search → #id mode",
      },
      {
        label: "Regex metadata scan",
        when: "You want to scan metadata fields (e.g. source filename, chunk text preview).",
        why: "Pattern-matches stored metadata keys and values — useful for finding all chunks from a specific document.",
        where: "Collection → Search → Regex mode",
      },
      {
        label: "Ask tab — LLM synthesis",
        when: "You want a natural-language answer synthesised from multiple chunks.",
        why: "Embeds your question, retrieves top-K chunks, expands context via the document graph, sends everything to an LLM. Every answer ships with a Proof-Carrying Receipt.",
        where: "Collection → Ask",
      },
      {
        label: "Global search",
        when: "You want to search across all collections at once.",
        why: "Runs the same semantic search but without a namespace filter.",
        where: "Sidebar → Search",
      },
    ],
  },
  {
    id: "add",
    icon: "↑",
    title: "Add documents",
    stripe: "border-emerald-500",
    badge: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/20",
    header: "text-emerald-600 dark:text-emerald-400",
    items: [
      {
        label: "Upload tab — ingest a file",
        when: "You have a PDF, DOCX, TXT, or Markdown file to ingest.",
        why: "Parses the file, splits it into overlapping chunks, embeds each chunk, stores vectors in the collection, saves text in the metadata sidecar, and builds a Document→Chunk knowledge graph.",
        where: "Collection → Upload",
      },
      {
        label: "Question suggester",
        when: "After a successful upload, you want to know what questions to ask.",
        why: "Sends the chunk previews to your LLM and returns 8 suggested questions. Click any to jump to the Ask tab with it pre-filled.",
        where: "Collection → Upload → ✦ Generate 8 questions",
      },
    ],
  },
  {
    id: "verify",
    icon: "◆",
    title: "Prove integrity",
    stripe: "border-purple-500",
    badge: "bg-purple-500/10 text-purple-600 dark:text-purple-400 border-purple-500/20",
    header: "text-purple-600 dark:text-purple-400",
    items: [
      {
        label: "Proof-Carrying Answers",
        when: "You need to prove exactly what an AI answer was based on — for audit, legal, or regulatory defense.",
        why: "Every Ask answer ships with a signed receipt: SHA-256 content hash of each cited chunk, global BLAKE3 state hash at answer time, the answer's own hash, and a self-fingerprint. Anyone with events.log can independently verify. (EU AI Act Article 12.)",
        where: "Collection → Ask → 🔏 Proof-carrying receipt",
      },
      {
        label: "Compliance Pack",
        when: "An auditor or regulator asks for evidence — SOC 2, HIPAA, EU AI Act, or GDPR.",
        why: "Assembles a signed evidence bundle: integrity attestation, tamper status vs. baseline, right-to-erasure certificates, and all answer-provenance receipts — mapped to regulatory controls. Self-verifying via SHA-256.",
        where: "Collection → Compliance → Generate pack",
      },
      {
        label: "Verify tab",
        when: "You want to prove a specific collection hasn't been tampered with.",
        why: "Computes SHA-256(sorted event IDs for this namespace) as a reproducible namespace proof hash. Also shows the global BLAKE3 state hash — both can be reproduced independently from events.log.",
        where: "Collection → Verify",
      },
      {
        label: "Certify — Proof Certificate",
        when: "You need a shareable, signed document proving the state of a collection at a point in time.",
        why: "Bundles the namespace hash, global BLAKE3 hash, record/event counts, and a SHA-256 self-certification fingerprint into downloadable JSON and a printable PDF certificate.",
        where: "Collection → Certify → Proof Certificate",
      },
      {
        label: "Certify — Tamper Detection",
        when: "You want an ongoing alert if the collection changes unexpectedly.",
        why: "Saves the current namespace hash as a baseline in your browser. Polls the live hash every 5 seconds and shows MATCH ✓ or MISMATCH ✗.",
        where: "Collection → Certify → Tamper Detection",
      },
      {
        label: "Proof page — global integrity",
        when: "You want the top-level global integrity proof for the entire node.",
        why: "Shows the BLAKE3 Merkle root over all applied events — the single number that summarises the complete state of the node.",
        where: "Sidebar → Proof",
      },
      {
        label: "Audit — Third-Party tab",
        when: "An auditor needs to independently verify the audit trail without access to internal tooling.",
        why: "Read-only view: state proof, event lookup, and provenance search.",
        where: "Sidebar → Audit Trail → Third-Party",
      },
    ],
  },
  {
    id: "compliance",
    icon: "⊛",
    title: "Compliance & erasure",
    stripe: "border-amber-500",
    badge: "bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/20",
    header: "text-amber-600 dark:text-amber-400",
    items: [
      {
        label: "GDPR tab — right to erasure",
        when: "A user exercises their right to erasure (GDPR Article 17) and you need to delete their data.",
        why: "Shows all records in the namespace with metadata. Select records (or filter to encrypted-only), confirm, and erase. Each deletion fires a DeleteRecord event permanently recorded in the BLAKE3 audit chain.",
        where: "Collection → GDPR",
      },
      {
        label: "GDPR — ShredKey (crypto-erasure)",
        when: "Records were inserted with per-record encryption keys (InsertRecordEncrypted).",
        why: "Destroying the key makes ciphertext unrecoverable without mutating the audit chain. Requires a backend endpoint — contact your system administrator.",
        where: "Collection → GDPR → encrypted badge",
      },
      {
        label: "Audit Trail",
        when: "You need a chronological record of every mutation for a compliance report.",
        why: "Lists every event with ID, type, timestamp, and affected record/node IDs. Exportable.",
        where: "Sidebar → Audit Trail",
      },
    ],
  },
  {
    id: "analyze",
    icon: "⬡",
    title: "Analyze & explore",
    stripe: "border-violet-500",
    badge: "bg-violet-500/10 text-violet-600 dark:text-violet-400 border-violet-500/20",
    header: "text-violet-600 dark:text-violet-400",
    items: [
      {
        label: "Eval tab — retrieval quality",
        when: "You want to measure how well your chunking and embedding works for retrieval.",
        why: "Paste ground-truth QA pairs (JSON or CSV). For each question it embeds, retrieves top-K chunks, uses the expected answer as an oracle to judge relevance, then computes Precision@K and MRR. Also finds orphaned chunks.",
        where: "Collection → Eval",
      },
      {
        label: "Contradictions tab",
        when: "You suspect your collection contains conflicting or contradictory statements.",
        why: "Negates each record's embedding and searches for nearest neighbors. cos(v_a, v_b) = 1 − L2²(−v_a, v_b)/2, so low scores from the negated search = semantic opposites. Streams results as they come in.",
        where: "Collection → Contradictions",
      },
      {
        label: "Diff tab — compare collections",
        when: "You want to compare two collections (e.g. staging vs. production, before vs. after a migration).",
        why: "Fetches the namespace-audit for both collections and computes the record/node ID set difference. Shows which records are only in A, only in B, or common. Also compares namespace proof hashes.",
        where: "Collection → Diff",
      },
      {
        label: "Graph tab — knowledge graph",
        when: "You want to visualise how documents, chunks, and entities are connected.",
        why: "Shows the knowledge graph for this collection — Document→Chunk edges from ingest, plus any entity nodes added via the graph API.",
        where: "Collection → Graph",
      },
      {
        label: "Documents tab",
        when: "You want to browse which documents are in the collection and see their chunks.",
        why: "Lists ingested documents with their chunk counts, source filenames, and metadata previews.",
        where: "Collection → Documents",
      },
    ],
  },
  {
    id: "operate",
    icon: "◉",
    title: "Operate the node",
    stripe: "border-zinc-500",
    badge: "bg-zinc-500/10 text-zinc-600 dark:text-zinc-400 border-zinc-500/20",
    header: "text-zinc-500 dark:text-zinc-400",
    items: [
      {
        label: "Snapshots",
        when: "You want to back up or restore the full state of the node.",
        why: "Downloads or uploads a V6 snapshot (binary, ~8 KB + data). Snapshots encode the full vector store, graph, and namespace registry. Restore replays the snapshot into a fresh kernel.",
        where: "Sidebar → Snapshots",
      },
      {
        label: "Metrics",
        when: "You want to monitor node health over time (record count growth, event log height, latency).",
        why: "Real-time time-series view of health metrics. Refreshes every 2 s.",
        where: "Sidebar → Metrics",
      },
      {
        label: "Logs",
        when: "You want to watch the raw event log stream.",
        why: "Shows every event as it is applied — useful for debugging ingestion pipelines.",
        where: "Sidebar → Logs",
      },
      {
        label: "Cluster page",
        when: "You're running a multi-node Raft cluster and want to see node health and state hash convergence.",
        why: "Shows each node's role (leader/follower), commit index, state hash, and whether all nodes have converged. Available in cluster mode only.",
        where: "Sidebar → Cluster",
      },
      {
        label: "Settings — embed & LLM config",
        when: "You need to configure your embedding model or LLM provider.",
        why: "Sets the provider (Ollama, OpenAI, Groq, Cohere, custom), model, API key, endpoint, chunk size, and overlap. Saved in browser localStorage — not sent to the server.",
        where: "Sidebar → Settings",
      },
    ],
  },
];

const QUICKSTART = [
  { n: 1, text: "Open Settings → configure an embedding model. Ollama is free and fully local." },
  { n: 2, text: "Create a project + collection (+ button next to Projects in the sidebar)." },
  { n: 3, text: "Go to Upload, drop in a PDF, and click Ingest document." },
  { n: 4, text: "After ingestion, click ✦ Generate 8 questions and pick one." },
  { n: 5, text: "Switch to Ask — your question is pre-filled. Press Enter to get a sourced answer." },
  { n: 6, text: "Go to Certify → Generate to get a downloadable or printable proof certificate." },
];

const TABS = [
  { name: "Search", icon: "⊙", cat: "find",       summary: "Vector, ID, regex, or raw float search" },
  { name: "Upload", icon: "↑",  cat: "add",        summary: "Ingest PDF / DOCX / TXT with auto-chunking" },
  { name: "Ask",    icon: "?",  cat: "find",       summary: "LLM Q&A over your collection, with receipts" },
  { name: "Documents", icon: "▤", cat: "analyze",  summary: "Browse ingested docs and their chunks" },
  { name: "Graph",  icon: "⬡", cat: "analyze",    summary: "Knowledge graph — doc→chunk→entity" },
  { name: "Verify", icon: "◆", cat: "verify",     summary: "SHA-256 namespace proof hash" },
  { name: "Eval",   icon: "≡",  cat: "analyze",   summary: "Precision@K and MRR with ground-truth QA pairs" },
  { name: "Certify", icon: "⊛", cat: "verify",    summary: "Signed certificate + tamper detection baseline" },
  { name: "GDPR",   icon: "⚠", cat: "compliance", summary: "Right-to-erasure with BLAKE3 audit proof" },
  { name: "Diff",   icon: "⇄", cat: "analyze",    summary: "Compare two namespaces by record ID set diff" },
  { name: "Contradictions", icon: "↕", cat: "analyze", summary: "Find semantically opposing chunks" },
  { name: "Compliance", icon: "⊛", cat: "compliance", summary: "One-button regulator evidence bundle" },
];

// ── Helpers ───────────────────────────────────────────────────────────────────

function highlight(text: string, q: string) {
  if (!q) return <>{text}</>;
  const idx = text.toLowerCase().indexOf(q.toLowerCase());
  if (idx === -1) return <>{text}</>;
  return (
    <>
      {text.slice(0, idx)}
      <mark className="bg-[var(--v-accent-muted)] text-[var(--v-accent)] rounded-[2px] px-0.5 not-italic">
        {text.slice(idx, idx + q.length)}
      </mark>
      {text.slice(idx + q.length)}
    </>
  );
}

function matches(item: Item, q: string) {
  const s = q.toLowerCase();
  return (
    item.label.toLowerCase().includes(s) ||
    item.when.toLowerCase().includes(s) ||
    item.why.toLowerCase().includes(s) ||
    item.where.toLowerCase().includes(s)
  );
}

// ── Components ────────────────────────────────────────────────────────────────

function ItemCard({ item, cat, q }: { item: Item; cat: Category; q: string }) {
  return (
    <div
      className={`relative bg-card border border-border rounded-lg overflow-hidden pl-4 pr-5 py-4 flex flex-col gap-3`}
    >
      {/* left color stripe */}
      <div className={`absolute left-0 top-0 bottom-0 w-[3px] ${cat.stripe}`} />

      <div className="flex items-center justify-between gap-3 flex-wrap">
        <p className="text-sm font-semibold text-foreground leading-snug">
          {highlight(item.label, q)}
        </p>
        <span
          className={`text-[10px] font-medium px-2 py-0.5 rounded-full border shrink-0 ${cat.badge}`}
        >
          {cat.title}
        </span>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
        <div className="flex flex-col gap-1">
          <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/60">When</p>
          <p className="text-xs text-muted-foreground leading-relaxed">{highlight(item.when, q)}</p>
        </div>
        <div className="flex flex-col gap-1">
          <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/60">How it works</p>
          <p className="text-xs text-muted-foreground leading-relaxed">{highlight(item.why, q)}</p>
        </div>
        <div className="flex flex-col gap-1">
          <p className="text-[9px] font-semibold uppercase tracking-widest text-muted-foreground/60">Where</p>
          <p className="text-xs font-mono text-muted-foreground leading-relaxed">{highlight(item.where, q)}</p>
        </div>
      </div>
    </div>
  );
}

function QuickStart() {
  return (
    <section className="bg-card border border-border rounded-xl p-5 flex flex-col gap-4">
      <div className="flex items-center gap-2">
        <span className="text-[10px] font-bold uppercase tracking-widest text-muted-foreground/60">Quick start</span>
        <span className="h-px flex-1 bg-border" />
        <span className="text-[10px] text-muted-foreground">PDF → Q&amp;A in 5 min</span>
      </div>
      <ol className="flex flex-col gap-0">
        {QUICKSTART.map(({ n, text }, i) => (
          <li key={n} className="flex gap-3 items-stretch">
            {/* step number + connector */}
            <div className="flex flex-col items-center shrink-0 w-7">
              <div className="w-6 h-6 rounded-full bg-[var(--v-accent-muted)] border border-[var(--v-accent)]/30 flex items-center justify-center shrink-0">
                <span className="text-[10px] font-bold font-mono text-[var(--v-accent)]">{n}</span>
              </div>
              {i < QUICKSTART.length - 1 && (
                <div className="w-px flex-1 bg-border my-1" />
              )}
            </div>
            <p className="text-sm text-muted-foreground leading-relaxed pb-3">{text}</p>
          </li>
        ))}
      </ol>
    </section>
  );
}

function TabCheatSheet() {
  const catMap = Object.fromEntries(CATEGORIES.map((c) => [c.id, c]));

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <span className="text-[10px] font-bold uppercase tracking-widest text-muted-foreground/60">Collection tabs</span>
        <span className="h-px flex-1 bg-border" />
      </div>
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-2">
        {TABS.map((t) => {
          const cat = catMap[t.cat];
          return (
            <div
              key={t.name}
              className={`relative bg-card border border-border rounded-lg pl-3 pr-3 py-2.5 overflow-hidden`}
            >
              <div className={`absolute left-0 top-0 bottom-0 w-[3px] ${cat?.stripe ?? "border-border"}`} />
              <p className="text-xs font-semibold text-foreground">
                <span className="font-mono text-muted-foreground mr-1.5">{t.icon}</span>
                {t.name}
              </p>
              <p className="text-[10px] text-muted-foreground leading-relaxed mt-0.5">{t.summary}</p>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function CategorySection({
  cat,
  q,
  sectionRef,
}: {
  cat: Category;
  q: string;
  sectionRef: (el: HTMLElement | null) => void;
}) {
  const visibleItems = q ? cat.items.filter((item) => matches(item, q)) : cat.items;
  if (q && visibleItems.length === 0) return null;

  return (
    <section id={`section-${cat.id}`} ref={sectionRef} className="flex flex-col gap-3 scroll-mt-4">
      <div className="flex items-center gap-2.5">
        <span className={`text-sm font-semibold ${cat.header}`}>
          {cat.icon} {cat.title}
        </span>
        <span className="h-px flex-1 bg-border" />
        <span className="text-[10px] text-muted-foreground">
          {visibleItems.length}/{cat.items.length}
        </span>
      </div>
      <div className="flex flex-col gap-2">
        {visibleItems.map((item) => (
          <ItemCard key={item.label} item={item} cat={cat} q={q} />
        ))}
      </div>
    </section>
  );
}

function SearchBar({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div className="relative w-full max-w-sm">
      <span className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground/50 text-xs pointer-events-none select-none">
        ⊙
      </span>
      <input
        type="search"
        placeholder="Search features…"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full pl-8 pr-3 py-2 text-sm bg-card border border-border rounded-lg
                   text-foreground placeholder:text-muted-foreground/40
                   focus:outline-none focus:ring-2 focus:ring-[var(--v-accent-ring)]
                   focus:border-[var(--v-accent)]/40"
      />
      {value && (
        <button
          onClick={() => onChange("")}
          className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground/50
                     hover:text-muted-foreground text-xs px-1"
          aria-label="Clear search"
        >
          ✕
        </button>
      )}
    </div>
  );
}

function Sidebar({
  activeId,
  onNav,
}: {
  activeId: string;
  onNav: (id: string) => void;
}) {
  return (
    <nav className="flex flex-col gap-0.5 sticky top-4">
      <p className="text-[9px] font-bold uppercase tracking-widest text-muted-foreground/50 px-2 mb-2">
        By goal
      </p>
      {CATEGORIES.map((cat) => (
        <button
          key={cat.id}
          onClick={() => onNav(cat.id)}
          className={`flex items-center gap-2.5 px-2 py-2 rounded-md text-left transition-colors w-full group
            ${activeId === cat.id
              ? "bg-[var(--v-accent-muted)] text-foreground"
              : "hover:bg-muted/50 text-muted-foreground hover:text-foreground"
            }`}
        >
          <span
            className={`w-2 h-2 rounded-full shrink-0 ${cat.stripe.replace("border-", "bg-")}`}
          />
          <span className="text-xs font-medium leading-snug">{cat.title}</span>
          <span className="ml-auto text-[10px] font-mono text-muted-foreground/40 group-hover:text-muted-foreground/60">
            {cat.items.length}
          </span>
        </button>
      ))}

      <div className="h-px bg-border my-3" />

      <Link
        href="/settings"
        className="flex items-center gap-2.5 px-2 py-2 rounded-md text-muted-foreground
                   hover:bg-muted/50 hover:text-foreground transition-colors text-xs"
      >
        <span className="w-2 h-2 shrink-0" />
        ⚙ Settings
      </Link>
    </nav>
  );
}

// ── Page ─────────────────────────────────────────────────────────────────────

export default function HelpPage() {
  const [search, setSearch] = useState("");
  const [activeId, setActiveId] = useState("find");
  const sectionRefs = useRef<Map<string, HTMLElement>>(new Map());

  const registerRef = useCallback(
    (id: string) => (el: HTMLElement | null) => {
      if (el) sectionRefs.current.set(id, el);
    },
    []
  );

  // IntersectionObserver — update sidebar active section while scrolling
  useEffect(() => {
    if (search) return;
    const obs = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            const id = entry.target.id.replace("section-", "");
            setActiveId(id);
          }
        }
      },
      { rootMargin: "-40% 0px -55% 0px" }
    );
    sectionRefs.current.forEach((el) => obs.observe(el));
    return () => obs.disconnect();
  }, [search]);

  const scrollToSection = useCallback((id: string) => {
    setActiveId(id);
    const el = document.getElementById(`section-${id}`);
    el?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, []);

  const totalResults = search
    ? CATEGORIES.reduce(
        (acc, cat) => acc + cat.items.filter((item) => matches(item, search)).length,
        0
      )
    : null;

  return (
    <div className="flex flex-col gap-5 w-full max-w-[1400px] py-1">

      {/* ── Header ─────────────────────────────────────────────────────── */}
      <div className="flex items-center justify-between gap-4 flex-wrap">
        <div>
          <h1 className="text-lg font-bold text-foreground tracking-tight">Feature Guide</h1>
          <p className="text-xs text-muted-foreground mt-0.5">
            What to use, when, and why.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <SearchBar value={search} onChange={setSearch} />
        </div>
      </div>

      {/* ── Mobile category pills ───────────────────────────────────────── */}
      {!search && (
        <div className="lg:hidden flex gap-1.5 overflow-x-auto pb-1 -mx-1 px-1">
          {CATEGORIES.map((cat) => (
            <button
              key={cat.id}
              onClick={() => scrollToSection(cat.id)}
              className={`shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-medium border transition-colors
                ${activeId === cat.id
                  ? `${cat.badge} border-current`
                  : "bg-card border-border text-muted-foreground hover:text-foreground"
                }`}
            >
              <span className={`w-1.5 h-1.5 rounded-full ${cat.stripe.replace("border-", "bg-")}`} />
              {cat.title}
            </button>
          ))}
        </div>
      )}

      {/* ── Search results meta ─────────────────────────────────────────── */}
      {search && (
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">
            {totalResults === 0
              ? "No results"
              : `${totalResults} result${totalResults === 1 ? "" : "s"} for `}
          </span>
          {totalResults !== 0 && (
            <span className="text-xs font-medium text-foreground">"{search}"</span>
          )}
          <button
            onClick={() => setSearch("")}
            className="text-xs text-muted-foreground hover:text-foreground underline underline-offset-2 ml-1"
          >
            Clear
          </button>
        </div>
      )}

      {/* ── Two-column layout ───────────────────────────────────────────── */}
      <div className="flex gap-7 items-start">

        {/* Sidebar — desktop only, not shown during search */}
        {!search && (
          <aside className="hidden lg:block w-44 shrink-0">
            <Sidebar activeId={activeId} onNav={scrollToSection} />
          </aside>
        )}

        {/* Main content */}
        <div className={`flex flex-col gap-6 min-w-0 flex-1`}>

          {/* Quick start + tab cheat sheet — only when not searching */}
          {!search && (
            <>
              <QuickStart />
              <TabCheatSheet />
              <div className="flex items-center gap-2">
                <span className="text-[10px] font-bold uppercase tracking-widest text-muted-foreground/50">By goal</span>
                <span className="h-px flex-1 bg-border" />
              </div>
            </>
          )}

          {/* Goal sections */}
          {CATEGORIES.map((cat) => (
            <CategorySection
              key={cat.id}
              cat={cat}
              q={search}
              sectionRef={registerRef(cat.id)}
            />
          ))}

          {/* Footer note */}
          {!search && (
            <div className="rounded-xl border border-border bg-card/60 px-5 py-4 text-xs text-muted-foreground leading-relaxed">
              <strong className="text-muted-foreground font-semibold">About Valori Kernel: </strong>
              All vectors are stored in Q16.16 fixed-point. Distances are L² squared — for
              unit-normalized embeddings,{" "}
              <code className="font-mono bg-muted px-1.5 py-0.5 rounded text-[11px]">
                cosine = 1 − score × 32768
              </code>
              . Every mutation is BLAKE3-chained into an append-only audit log. Namespaces
              (collections) are 16-bit integer IDs; the namespace label is stored in the UI only.
              The Rust binary{" "}
              <code className="font-mono bg-muted px-1.5 py-0.5 rounded text-[11px]">
                valori-verify
              </code>{" "}
              can independently replay any events.log and reproduce the final state hash without
              this UI.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
