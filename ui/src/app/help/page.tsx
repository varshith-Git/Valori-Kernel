"use client";

import Link from "next/link";
import { useState } from "react";

// --- Data ---------------------------------------------------------------------

const GOALS = [
  {
    id: "find",
    icon: "⊙",
    title: "Find information",
    color: "border-blue-900/60 bg-blue-950/20",
    accent: "text-blue-600 dark:text-blue-400",
    items: [
      {
        label: "Search tab → Semantic",
        when: "You have a natural-language question or concept (e.g. &quot;treatment side effects&quot;).",
        why: "Converts your text to a vector using the configured embedding model, then finds the nearest chunks in the collection.",
        where: "Collection → Search → Text query sub-mode",
      },
      {
        label: "Search tab → Raw vector",
        when: "You already have a float vector (e.g. from your application code).",
        why: "Bypasses embedding — sends the vector directly to Valori's HNSW/brute-force index.",
        where: "Collection → Search → Raw vector sub-mode",
      },
      {
        label: "Search tab → #id",
        when: "You know the exact record ID you want to inspect.",
        why: "Direct lookup — no vector search, instant result.",
        where: "Collection → Search → #id mode",
      },
      {
        label: "Search tab → Regex",
        when: "You want to scan metadata fields (e.g. source filename, chunk text preview).",
        why: "Pattern-matches stored metadata keys and values — useful for finding all chunks from a specific document.",
        where: "Collection → Search → Regex mode",
      },
      {
        label: "Ask tab",
        when: "You want a natural-language answer synthesized from multiple chunks.",
        why: "Embeds your question, retrieves the top-K chunks, expands context via the document graph, then sends everything to an LLM. Uses your configured LLM provider (Ollama, OpenAI, Groq, etc.). Every answer ships with a Proof-Carrying Receipt (see Prove integrity).",
        where: "Collection → Ask",
      },
      {
        label: "Global search (/search)",
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
    color: "border-emerald-900/60 bg-emerald-950/20",
    accent: "text-emerald-600 dark:text-emerald-400",
    items: [
      {
        label: "Upload tab",
        when: "You have a PDF, DOCX, TXT, or Markdown file to ingest.",
        why: "Parses the file, splits it into overlapping chunks, embeds each chunk, stores vectors in the collection, saves text in the metadata sidecar, and builds a Document→Chunk knowledge graph.",
        where: "Collection → Upload",
      },
      {
        label: "Question suggester (post-upload)",
        when: "After a successful upload, you want to know what questions to ask.",
        why: "Sends the chunk previews to your LLM and returns 8 suggested questions. Click any question to jump to the Ask tab with it pre-filled.",
        where: "Collection → Upload → ✦ Generate 8 questions (appears after upload completes)",
      },
    ],
  },
  {
    id: "verify",
    icon: "◆",
    title: "Prove integrity",
    color: "border-purple-900/60 bg-purple-950/20",
    accent: "text-purple-600 dark:text-purple-400",
    items: [
      {
        label: "Proof-Carrying Answers (Ask tab)",
        when: "You need to prove later exactly what an AI answer was based on — for audit, legal, or regulatory defense.",
        why: "Every Ask answer ships with a signed receipt: the SHA-256 content hash of each cited chunk, the global BLAKE3 state hash at answer time, the answer's own hash, and a self-fingerprint. With a copy of events.log, anyone can prove the answer was grounded in exactly those unaltered chunks. Download as JSON or print as a PDF certificate. (EU AI Act Article 12.)",
        where: "Collection → Ask → 🔏 Proof-carrying receipt (under each answer)",
      },
      {
        label: "Compliance Pack (Compliance tab)",
        when: "An auditor or regulator asks for evidence — SOC 2, HIPAA, EU AI Act, or a GDPR audit.",
        why: "One button assembles a signed evidence bundle for the collection: integrity attestation (namespace + global hashes, counts), tamper status vs. your saved baseline, all right-to-erasure certificates, and all answer-provenance receipts — mapped to the specific regulatory controls. Self-verifying via SHA-256; download JSON or print a multi-section PDF.",
        where: "Collection → Compliance → Generate pack",
      },
      {
        label: "Verify tab",
        when: "You want to prove that a specific collection hasn't been tampered with.",
        why: "Computes SHA-256(sorted event IDs for this namespace) as a reproducible namespace proof hash. Also shows the global BLAKE3 state hash. Anyone with a copy of events.log can reproduce both numbers independently.",
        where: "Collection → Verify",
      },
      {
        label: "Certify tab → Proof Certificate",
        when: "You need a shareable, signed document proving the state of a collection at a point in time.",
        why: "Bundles the namespace hash, global BLAKE3 hash, record/event counts, and a SHA-256 self-certification fingerprint into a downloadable JSON. Also generates a printable PDF certificate.",
        where: "Collection → Certify → Proof Certificate",
      },
      {
        label: "Certify tab → Tamper Detection",
        when: "You want an ongoing alert if the collection changes unexpectedly.",
        why: "Saves the current namespace hash as a baseline in your browser. Polls the live hash every 5 seconds and shows MATCH ✓ or MISMATCH ✗. Useful for compliance monitoring.",
        where: "Collection → Certify → Tamper Detection",
      },
      {
        label: "Proof page (/)",
        when: "You want the top-level global integrity proof for the entire node.",
        why: "Shows the BLAKE3 Merkle root over all applied events — the single number that summarises the complete state of the node.",
        where: "Sidebar → Proof",
      },
      {
        label: "Auditor Portal (/auditor)",
        when: "A third party (auditor, regulator) needs to independently verify the audit trail.",
        why: "Self-service portal — paste an event log, get a verification report. Requires no access to internal tooling.",
        where: "Sidebar → Auditor Portal",
      },
    ],
  },
  {
    id: "compliance",
    icon: "⊛",
    title: "Compliance & erasure",
    color: "border-amber-900/60 bg-amber-950/20",
    accent: "text-amber-600 dark:text-amber-400",
    items: [
      {
        label: "GDPR tab",
        when: "A user exercises their right to erasure (GDPR Article 17) and you need to delete their data.",
        why: "Shows all records in the namespace with their metadata. Select records (or filter to encrypted-only), confirm, and erase. Each deletion fires a DeleteRecord event that is permanently recorded in the BLAKE3 audit chain — you can prove erasure happened without exposing what was erased.",
        where: "Collection → GDPR",
      },
      {
        label: "GDPR tab → ShredKey note",
        when: "Records were inserted with per-record encryption keys (InsertRecordEncrypted).",
        why: "Crypto-erasure: destroying the key makes ciphertext unrecoverable without mutating the audit chain. The UI shows the key prefix and explains the ShredKey path. Requires a backend endpoint not yet exposed — contact your system administrator.",
        where: "Collection → GDPR → encrypted badge",
      },
      {
        label: "Audit Trail (/audit)",
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
    color: "border-input/60 bg-card/40",
    accent: "text-accent-foreground",
    items: [
      {
        label: "Eval tab",
        when: "You want to measure how well your chunking/embedding is working for retrieval.",
        why: "Paste ground-truth QA pairs (JSON or CSV). For each question it embeds, retrieves top-K chunks, uses the expected answer as an oracle to judge relevance, then computes Precision@K and MRR. Also finds orphaned chunks — records that were never retrieved for any query.",
        where: "Collection → Eval",
      },
      {
        label: "Contradictions tab",
        when: "You suspect your collection contains conflicting or contradictory statements.",
        why: "Negates each record's embedding and searches for nearest neighbors. For unit-normalized vectors, cos(v_a, v_b) = 1 − L2²(−v_a, v_b)/2, so low scores from the negated search = semantic opposites. Streams results as they come in.",
        where: "Collection → Contradictions",
      },
      {
        label: "Diff tab",
        when: "You want to compare two collections (e.g. staging vs. production, before vs. after a migration).",
        why: "Fetches the namespace-audit for both collections and computes the record/node ID set difference. Shows which records are only in A, only in B, or common. Also compares namespace proof hashes.",
        where: "Collection → Diff",
      },
      {
        label: "Graph tab",
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
    color: "border-input/60 bg-card/40",
    accent: "text-muted-foreground",
    items: [
      {
        label: "Snapshots (/snapshots)",
        when: "You want to back up or restore the full state of the node.",
        why: "Downloads or uploads a V6 snapshot (binary, ~8 KB + data). Snapshots encode the full vector store, graph, and namespace registry. Restore replays the snapshot into a fresh kernel.",
        where: "Sidebar → Snapshots",
      },
      {
        label: "Metrics (/metrics)",
        when: "You want to monitor node health over time (record count growth, event log height, latency).",
        why: "Real-time time-series view of health metrics. Refreshes every 2 s.",
        where: "Sidebar → Metrics",
      },
      {
        label: "Logs (/logs)",
        when: "You want to watch the raw event log stream.",
        why: "Shows every event as it is applied — useful for debugging ingestion pipelines.",
        where: "Sidebar → Logs",
      },
      {
        label: "Cluster page (/cluster)",
        when: "You're running a multi-node Raft cluster and want to see node health and state hash convergence.",
        why: "Shows each node's role (leader/follower), commit index, state hash, and whether all nodes have converged to the same hash. Available in cluster mode only.",
        where: "Sidebar → Cluster (visible only in cluster mode)",
      },
      {
        label: "Settings (/settings)",
        when: "You need to configure your embedding model or LLM provider.",
        why: "Sets the provider (Ollama, OpenAI, Groq, Cohere, custom), model, API key, endpoint, chunk size, and overlap. Saved in browser localStorage — not sent to the server.",
        where: "Sidebar → Settings",
      },
    ],
  },
];

const QUICKSTART = [
  { step: 1, text: "Open Settings and configure your embedding model (Ollama is free and local)." },
  { step: 2, text: "Create a project and collection from the sidebar (+ button next to Projects)." },
  { step: 3, text: "Go to Upload, drop in a PDF, and click Ingest document." },
  { step: 4, text: "After ingestion, click ✦ Generate 8 questions and pick one to ask." },
  { step: 5, text: "Switch to Ask — your question is pre-filled. Press Enter to get an answer with sources." },
  { step: 6, text: "Go to Certify and click Generate → to get a proof certificate you can download or print." },
];

// --- Components ---------------------------------------------------------------

function QuickStart() {
  return (
    <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-4">
      <p className="text-sm font-semibold text-card-foreground">Quick start — PDF to Q&A in 5 minutes</p>
      <ol className="flex flex-col gap-3">
        {QUICKSTART.map(({ step, text }) => (
          <li key={step} className="flex items-start gap-3">
            <span className="w-5 h-5 rounded-full bg-muted text-accent-foreground text-[10px] font-mono font-bold flex items-center justify-center flex-shrink-0 mt-0.5">
              {step}
            </span>
            <p className="text-sm text-muted-foreground leading-relaxed">{text}</p>
          </li>
        ))}
      </ol>
    </div>
  );
}

function GoalSection({
  goal,
}: {
  goal: typeof GOALS[number];
}) {
  const [open, setOpen] = useState(true);

  return (
    <div className={`rounded-xl border ${goal.color} overflow-hidden`}>
      <button
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center justify-between px-5 py-4 hover:bg-white/5 transition-colors"
      >
        <div className="flex items-center gap-3">
          <span className={`text-base ${goal.accent}`}>{goal.icon}</span>
          <span className={`text-sm font-semibold ${goal.accent}`}>{goal.title}</span>
          <span className="text-[10px] text-muted-foreground font-mono">{goal.items.length} features</span>
        </div>
        <span className="text-muted-foreground text-xs">{open ? "▲" : "▼"}</span>
      </button>

      {open && (
        <div className="flex flex-col divide-y divide-border/60 border-t border-border/60">
          {goal.items.map((item) => (
            <div key={item.label} className="px-5 py-4 flex flex-col gap-2">
              <p className="text-xs font-semibold text-accent-foreground">{item.label}</p>
              <div className="grid grid-cols-1 gap-1.5 sm:grid-cols-3">
                <div className="flex flex-col gap-0.5">
                  <p className="text-[9px] text-muted-foreground uppercase tracking-widest">When</p>
                  <p className="text-xs text-muted-foreground leading-relaxed">{item.when}</p>
                </div>
                <div className="flex flex-col gap-0.5">
                  <p className="text-[9px] text-muted-foreground uppercase tracking-widest">Why it works</p>
                  <p className="text-xs text-muted-foreground leading-relaxed">{item.why}</p>
                </div>
                <div className="flex flex-col gap-0.5">
                  <p className="text-[9px] text-muted-foreground uppercase tracking-widest">Where</p>
                  <p className="text-xs text-muted-foreground font-mono leading-relaxed">{item.where}</p>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function TabCheatSheet() {
  const tabs = [
    { name: "Search", icon: "⊙", summary: "Find records by vector similarity, ID, or regex" },
    { name: "Upload", icon: "↑", summary: "Ingest PDF/DOCX/TXT with auto-chunking + embedding" },
    { name: "Ask", icon: "?", summary: "Natural-language Q&A with LLM synthesis" },
    { name: "Documents", icon: "▤", summary: "Browse ingested documents and their chunks" },
    { name: "Graph", icon: "⬡", summary: "Visualise document→chunk and entity relationships" },
    { name: "Verify", icon: "◆", summary: "Compute SHA-256 namespace proof hash" },
    { name: "Eval", icon: "≡", summary: "Score retrieval with ground-truth QA pairs (Precision@K, MRR)" },
    { name: "Certify", icon: "⊛", summary: "Signed certificate + tamper detection baseline" },
    { name: "GDPR", icon: "⚠", summary: "Right-to-erasure workflow with BLAKE3 proof" },
    { name: "Diff", icon: "⇄", summary: "Compare two namespaces by record ID set difference" },
    { name: "Contradictions", icon: "↕", summary: "Find semantically opposing chunks via negated vector search" },
    { name: "Compliance", icon: "⊛", summary: "One-button regulator evidence bundle (EU AI Act / GDPR / SOC 2)" },
  ];

  return (
    <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-3">
      <p className="text-sm font-semibold text-card-foreground">Collection page tabs — at a glance</p>
      <div className="grid grid-cols-1 gap-1.5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        {tabs.map((t) => (
          <div
            key={t.name}
            className="flex items-center gap-3 rounded-lg bg-background border border-border px-3 py-2.5"
          >
            <span className="font-mono text-xs text-muted-foreground w-4 text-center">{t.icon}</span>
            <div>
              <p className="text-xs font-medium text-accent-foreground">{t.name}</p>
              <p className="text-[10px] text-muted-foreground leading-relaxed mt-0.5">{t.summary}</p>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

// --- Page ---------------------------------------------------------------------

export default function HelpPage() {
  return (
    <div className="flex flex-col gap-6 w-full max-w-[1600px] py-2">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-xl font-bold text-foreground">Feature Guide</h1>
          <p className="text-sm text-muted-foreground mt-1">
            What to use, when, and why. Start with Quick Start if you&apos;re new.
          </p>
        </div>
        <Link
          href="/settings"
          className="text-xs text-muted-foreground hover:text-muted-foreground transition-colors border border-border rounded px-3 py-1.5"
        >
          ⚙ Settings →
        </Link>
      </div>

      <QuickStart />
      <TabCheatSheet />

      <div className="flex flex-col gap-1">
        <p className="text-xs text-muted-foreground uppercase tracking-widest px-1">By goal</p>
      </div>

      {GOALS.map((goal) => (
        <GoalSection key={goal.id} goal={goal} />
      ))}

      {/* Footer note */}
      <div className="rounded-xl border border-border bg-card/50 px-5 py-4 text-xs text-muted-foreground leading-relaxed">
        <strong className="text-muted-foreground">About Valori Kernel:</strong> All vectors are stored in
        Q16.16 fixed-point. Distances are L² squared (not cosine) — for unit-normalized embeddings,{" "}
        <code className="font-mono bg-accent px-1 rounded">cosine = 1 − score × 32768</code>.
        Every mutation is BLAKE3-chained into an append-only audit log. Namespaces (collections) are
        16-bit integer IDs; the namespace label is stored in the UI only. The Rust binary{" "}
        <code className="font-mono bg-accent px-1 rounded">valori-verify</code> can
        independently replay any events.log and reproduce the final state hash without this UI.
      </div>
    </div>
  );
}
