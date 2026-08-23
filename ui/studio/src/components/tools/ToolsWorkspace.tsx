'use client'

// Ported from valori-kernel/ui's app/projects/[name]/[collection]/page.tsx —
// same CollectionHeader + flat-tabs-with-overflow-menus layout, same tab
// registry. Adapted for multi-tenancy: kernel routes to a dedicated URL per
// collection (`/projects/[name]/[collection]`); here one project IS one
// node, so collection switching is a dropdown embedded in the header
// instead of a route change, backed by `?collection=` in the URL for
// bookmarkability. Kernel's "Graph" analyze tab isn't ported here — this
// app already has a dedicated Graph page per project
// (`/cloud/projects/[id]/graph`), so it isn't duplicated as a tab too.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ChevronDown, ChevronRight, Users, Wrench, Terminal, Database, Layers, BookOpen, SlidersHorizontal } from 'lucide-react'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { useHealth } from '@/lib/hooks/useHealth'
import { useCollections } from '@/lib/hooks/useCollections'
import { useCollectionIndex } from '@/lib/hooks/useCollectionIndex'
import { useTransport } from '@/runtime/context'
import type { StudioCapabilities } from '@/runtime/capabilities'
import { cn } from '@/lib/utils'
import { MultiSearch } from '@/components/collections/MultiSearch'
import { DocumentUploadTab } from '@/components/collections/DocumentUploadTab'
import { BulkInsertTab } from '@/components/collections/BulkInsertTab'
import { VisualizeTab } from '@/components/collections/VisualizeTab'
import { AskTab } from '@/components/collections/AskTab'
import { DocumentsTab } from '@/components/collections/DocumentsTab'
import { TreeRagTab } from '@/components/collections/TreeRagTab'
import { CommunityTab } from '@/components/collections/CommunityTab'
import { EntityExtractionTab } from '@/components/collections/EntityExtractionTab'
import { VerifyTab } from '@/components/collections/VerifyTab'
import { EvalTab } from '@/components/collections/EvalTab'
import { CertifyTab } from '@/components/collections/CertifyTab'
import { GdprTab } from '@/components/collections/GdprTab'
import { DiffTab } from '@/components/collections/DiffTab'
import { ContradictionTab } from '@/components/collections/ContradictionTab'
import { CompliancePackTab } from '@/components/collections/CompliancePackTab'
import { IndexLifecycleTab } from '@/components/collections/IndexLifecycleTab'
import { TabShell } from '@/components/collections/TabShell'

const DEFAULT_NAMESPACE = 'default'

/* -- Tab registry (verbatim from kernel, minus the Graph tab) --------- */

const PRIMARY_TABS = [
    { value: 'search', label: 'Search', tip: 'Find records by semantic similarity, ID, or regex' },
    { value: 'upload', label: 'Upload', tip: 'Ingest PDF / DOCX / TXT with auto-chunking and embedding' },
    { value: 'bulk', label: 'Bulk Insert', tip: 'Insert multiple vectors at once from CSV or JSON' },
    { value: 'visualize', label: 'Visualize', tip: '2D PCA scatter plot of all vectors in this collection' },
    { value: 'ask', label: 'Ask', tip: 'Natural-language Q&A with LLM synthesis over top-K chunks' },
    { value: 'docs', label: 'Documents', tip: 'Browse ingested documents and their chunks' },
]

const ANALYZE_TABS = [
    { value: 'index', label: 'Index', tip: 'ANN index lifecycle: create, change, or remove the acceleration structure for this collection' },
    { value: 'treerag', label: 'Tree-RAG', tip: "Navigate a document's section tree by term frequency — line-cited answers + BLAKE3 receipt chain" },
    { value: 'community', label: 'Communities', tip: 'Label Propagation community detection + centroid search — find themes across the entire graph' },
    { value: 'entities', label: 'Entity Extract', tip: 'LLM extracts named entities + relationships from text and inserts them as graph nodes + edges' },
    { value: 'eval', label: 'Eval', tip: 'Score retrieval quality with ground-truth QA pairs: Precision@K, MRR' },
    { value: 'diff', label: 'Diff', tip: 'Compare two namespaces by record/node ID set difference' },
    { value: 'contradict', label: 'Contradictions', tip: 'Find semantically opposing chunks by negating embeddings' },
    { value: 'info', label: 'Info', tip: 'Collection metadata: namespace ID, vector dimension, storage details' },
]

const COMPLIANCE_TABS = [
    { value: 'verify', label: 'Verify', tip: 'Compute SHA-256 namespace proof hash — reproducible from events.log' },
    { value: 'certify', label: 'Certify', tip: 'Signed JSON + PDF proof certificate with tamper detection' },
    { value: 'gdpr', label: 'GDPR', tip: 'Right-to-erasure with BLAKE3-chained erasure certificate' },
    { value: 'compliance', label: 'Compliance', tip: 'Regulator evidence bundle (EU AI Act / GDPR / SOC 2)' },
]

type TabDef = { value: string; label: string; tip: string }

/* -- Group overflow dropdown (verbatim from kernel) -------------------- */

function GroupMenu({
    label,
    tabs,
    activeValue,
    onSelect,
}: {
    label: string
    tabs: TabDef[]
    activeValue: string
    onSelect: (v: string) => void
}) {
    const [open, setOpen] = useState(false)
    const ref = useRef<HTMLDivElement>(null)
    const activeTab = tabs.find((t) => t.value === activeValue)

    useEffect(() => {
        function onClickOutside(e: MouseEvent) {
            if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
        }
        document.addEventListener('mousedown', onClickOutside)
        return () => document.removeEventListener('mousedown', onClickOutside)
    }, [])

    return (
        <div ref={ref} className="relative">
            <button
                onClick={() => setOpen((v) => !v)}
                className={cn(
                    'inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-all',
                    activeTab ? 'bg-[var(--v-accent-muted)] text-foreground' : 'text-muted-foreground hover:bg-accent hover:text-card-foreground',
                )}
            >
                {activeTab ? activeTab.label : label}
                <ChevronDown size={13} className={cn('transition-transform', open && 'rotate-180')} />
            </button>

            {open && (
                <div className="absolute left-0 top-full z-50 mt-1.5 w-52 rounded-xl border border-input bg-card shadow-xl shadow-black/40 py-1 overflow-hidden">
                    {tabs.map((t) => (
                        <button
                            key={t.value}
                            title={t.tip}
                            onClick={() => {
                                onSelect(t.value)
                                setOpen(false)
                            }}
                            className={cn(
                                'w-full flex items-center gap-2 px-3 py-2 text-sm text-left transition-colors',
                                activeValue === t.value ? 'bg-[var(--v-accent-muted)] text-foreground' : 'text-muted-foreground hover:bg-accent hover:text-card-foreground',
                            )}
                        >
                            <ChevronRight size={12} className="shrink-0 text-muted-foreground" />
                            <span>{t.label}</span>
                        </button>
                    ))}
                </div>
            )}
        </div>
    )
}

/* -- Collection header (adapted from kernel: name is a dropdown here) -- */

const ICON_VARIANTS = [
    { Icon: Users, bg: 'bg-blue-500/10', color: 'text-blue-500' },
    { Icon: Wrench, bg: 'bg-rose-500/10', color: 'text-rose-500' },
    { Icon: Terminal, bg: 'bg-emerald-500/10', color: 'text-emerald-600 dark:text-emerald-400' },
    { Icon: Database, bg: 'bg-purple-500/10', color: 'text-purple-500' },
    { Icon: Layers, bg: 'bg-amber-500/10', color: 'text-amber-500' },
    { Icon: BookOpen, bg: 'bg-cyan-500/10', color: 'text-cyan-500' },
]

function getIconVariant(name: string) {
    let h = 0
    for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xffff
    return ICON_VARIANTS[h % ICON_VARIANTS.length]
}

function CollectionHeader({
    namespace,
    collections,
    onChangeCollection,
    dim,
    online,
    indexStatus,
    onViewDetails,
}: {
    namespace: string
    collections: string[]
    onChangeCollection: (name: string) => void
    dim: number | null
    online: boolean
    /** Live collection-specific index status from GET /v1/namespaces/{name}/index. */
    indexStatus: { active_type: string; status: string } | null
    onViewDetails: () => void
}) {
    const { Icon, bg, color } = getIconVariant(namespace)

    // Derive a compact index label: "HNSW", "IVF", "BQ", "None", or "Building…"
    let indexLabel = '—'
    if (indexStatus) {
        if (indexStatus.status === 'building') {
            indexLabel = `${indexStatus.active_type !== 'none' ? indexStatus.active_type.toUpperCase() + ' · ' : ''}Building…`
        } else if (indexStatus.active_type !== 'none') {
            indexLabel = indexStatus.active_type.toUpperCase()
        } else {
            indexLabel = 'None'
        }
    }

    const stats = [
        { label: 'Dimension', value: dim != null ? String(dim) : '—' },
        { label: 'Index', value: indexLabel },
        { label: 'Shards', value: '1' },
        {
            label: 'Status',
            value: online ? 'Healthy' : 'Unreachable',
            className: online ? 'text-emerald-600 dark:text-emerald-400' : 'text-amber-600 dark:text-amber-400',
            dot: online ? 'bg-emerald-500' : 'bg-amber-500',
        },
    ]

    return (
        <div className="rounded-xl border border-border bg-card px-5 py-4 flex items-center gap-4">
            <div className={cn('w-11 h-11 rounded-xl flex items-center justify-center shrink-0', bg)}>
                <Icon size={20} className={color} />
            </div>
            <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2.5 mb-2">
                    <div className="relative">
                        <select
                            value={namespace}
                            onChange={(e) => onChangeCollection(e.target.value)}
                            className="appearance-none rounded-md border-0 bg-transparent pr-6 py-0 text-lg font-semibold text-foreground focus:outline-none cursor-pointer -ml-0.5"
                        >
                            {!collections.includes(namespace) && <option value={namespace}>{namespace}</option>}
                            {!collections.includes(DEFAULT_NAMESPACE) && <option value={DEFAULT_NAMESPACE}>default</option>}
                            {collections.map((c) => (
                                <option key={c} value={c}>
                                    {c}
                                </option>
                            ))}
                        </select>
                        <ChevronDown size={13} className="pointer-events-none absolute right-0 top-1/2 -translate-y-1/2 text-muted-foreground" />
                    </div>
                    <span className="text-xs font-medium bg-[var(--v-accent-muted)] text-[var(--v-accent)] border border-[var(--v-accent)]/20 rounded-full px-2 py-0.5">
                        Collection
                    </span>
                    <span className="text-xs font-medium bg-muted text-muted-foreground rounded-full px-2 py-0.5 border border-border">
                        {collections.length} total
                    </span>
                </div>
                <div className="flex items-center gap-4 flex-wrap">
                    {stats.map(({ label, value, className, dot }) => (
                        <div key={label} className="flex items-center gap-1.5">
                            <span className="text-[11px] text-muted-foreground">{label}</span>
                            {dot && <span className={cn('w-1.5 h-1.5 rounded-full shrink-0', dot)} />}
                            <span className={cn('text-xs font-semibold text-foreground', className)}>{value}</span>
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
    )
}

/* -- Collection info panel (adapted from kernel) ----------------------- */

function CollectionInfo({ projectName, namespace, dim }: { projectName: string; namespace: string; dim: number | null }) {
    return (
        <TabShell>
            <div className="rounded-xl border border-border bg-card divide-y divide-border">
                <InfoRow label="Project" value={projectName} />
                <InfoRow label="Collection" value={namespace} mono />
                <InfoRow label="Dimension" value={dim != null ? String(dim) : '—'} />
                <InfoRow
                    label="Storage per record"
                    value={dim != null ? `${dim * 4} bytes` : '—'}
                    sub={dim != null ? `${dim} scalars × 4 B (Q16.16)` : undefined}
                />
                <InfoRow label="Search modes" value="Semantic · #id · Regex" />
                <InfoRow label="Pending modes" value="Text · Hybrid · Metadata" sub="requires embedding API" />
            </div>
        </TabShell>
    )
}

function InfoRow({ label, value, sub, mono }: { label: string; value: string; sub?: string; mono?: boolean }) {
    return (
        <div className="flex items-start justify-between px-4 py-3">
            <p className="text-sm text-muted-foreground">{label}</p>
            <div className="text-right">
                <span className={`text-sm ${mono ? 'font-mono' : ''} text-accent-foreground`}>{value}</span>
                {sub && <p className="text-xs text-muted-foreground mt-0.5">{sub}</p>}
            </div>
        </div>
    )
}

/* -- Page --------------------------------------------------------------- */

export function ToolsWorkspace({
    projectId,
    projectName,
    initialCollection,
    onCollectionChange,
    onMutate,
    capabilities,
    settingsHref,
    extraTabs,
}: {
    projectId: string
    projectName: string
    initialCollection?: string
    /** Called when the user switches collections — the host updates its own
     *  URL however it likes (e.g. a shallow router.replace with
     *  `?collection=`, to keep the choice bookmarkable) rather than this
     *  package assuming a Next.js router exists. */
    onCollectionChange?: (name: string) => void
    /** Called after a mutation (record delete) that may invalidate
     *  server-rendered data the host owns — the host decides how/whether to
     *  refresh (e.g. router.refresh()). */
    onMutate?: () => void
    /** Threaded down to the Upload tab — gates its client-embedding-fallback
     *  fields via capabilities.clientEmbeddingFallback. Omit to keep the
     *  server-pipeline-only behavior every host had before this. */
    capabilities?: StudioCapabilities
    /** Threaded down to the Upload tab's "configure →" link, shown only
     *  when clientEmbeddingFallback is on. */
    settingsHref?: string
    /** Host-supplied additional tabs (e.g. a host wiring in `<GraphView
     *  embedded />`) — merged into the named group's menu, rendered via
     *  `render()` when selected. Lets a host extend the shared tab set
     *  without a second tab-registry/workspace implementation. */
    extraTabs?: Array<{
        value: string
        label: string
        tip: string
        group: 'primary' | 'analyze' | 'compliance'
        render: () => React.ReactNode
    }>
}) {
    const transport = useTransport();
    const { dim: healthDim, online } = useHealth(projectId)
    // `collections` (canonical display names) drives the picker and header;
    // `namespace` state below is that same canonical name. Every tab that
    // actually queries the node needs the RAW namespace instead — a host
    // whose collections have no naming convention of their own (Cloud)
    // simply has raw === canonical for every entry, so this resolution is a
    // no-op there; a host with legacy-prefixed namespaces (Desktop Local)
    // needs it to target the right data. ToolsWorkspace still contains zero
    // prefix/separator logic of its own — see useCollections's own docs.
    const { collections, raw: rawCollections } = useCollections(projectId)
    const [namespace, setNamespace] = useState(initialCollection || DEFAULT_NAMESPACE)
    const currentCollectionRef = rawCollections.find((r) => r.name === namespace)
    const rawNamespace = currentCollectionRef?.rawNamespace ?? namespace
    // Prefer the collection-specific dimension from GET /v1/namespaces (available
    // even when no records exist). Fall back to health dim for legacy hosts that
    // don't include `dimension` in the namespace list.
    const dim = currentCollectionRef?.dimension ?? healthDim
    // Live collection-specific index status — drives the header badge and the
    // Index tab. This is a per-collection GET, not the project-wide /health index field.
    const { data: collectionIndexData } = useCollectionIndex(projectId, rawNamespace)
    const [activeTab, setActiveTab] = useState('search')
    const [pendingQuestion, setPendingQuestion] = useState('')

    const changeCollection = useCallback(
        (name: string) => {
            setNamespace(name)
            onCollectionChange?.(name)
        },
        [onCollectionChange],
    )

    const handleAskQuestion = useCallback((q: string) => {
        setPendingQuestion(q)
        setActiveTab('ask')
    }, [])

    const deleteRecord = async (id: number) => {
        const res = await fetch(transport.path(projectId, `/delete`), {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ id }),
        })
        if (!res.ok) {
            const body = (await res.json().catch(() => ({}))) as { error?: string }
            throw new Error(body.error ?? `Delete failed (${res.status})`)
        }
        onMutate?.()
    }

    const primaryTabs = useMemo(
        () => [...PRIMARY_TABS, ...(extraTabs?.filter((t) => t.group === 'primary') ?? [])],
        [extraTabs],
    )
    const analyzeTabs = useMemo(
        () => [...ANALYZE_TABS, ...(extraTabs?.filter((t) => t.group === 'analyze') ?? [])],
        [extraTabs],
    )
    const complianceTabs = useMemo(
        () => [...COMPLIANCE_TABS, ...(extraTabs?.filter((t) => t.group === 'compliance') ?? [])],
        [extraTabs],
    )

    return (
        <div className="flex flex-col gap-5 w-full">
            <CollectionHeader
                namespace={namespace}
                collections={collections}
                onChangeCollection={changeCollection}
                dim={dim}
                online={online}
                indexStatus={collectionIndexData ?? null}
                onViewDetails={() => setActiveTab('index')}
            />

            {/* Tab bar: primary + two named group menus — same shape as kernel's
                collection page. */}
            <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as string)}>
                <div className="flex items-center gap-2 flex-wrap mb-1">
                    <TabsList className="inline-flex bg-[#e4e8ec] dark:bg-zinc-800 rounded-lg p-1 h-auto gap-0.5">
                        {primaryTabs.map(({ value, label, tip }) => (
                            <TabsTrigger
                                key={value}
                                value={value}
                                title={tip}
                                className="rounded-md border-0 text-muted-foreground bg-transparent px-4 py-1.5 text-sm font-medium transition-all data-active:bg-white dark:data-active:bg-zinc-700 data-active:text-foreground data-active:shadow-sm hover:text-foreground"
                            >
                                {label}
                            </TabsTrigger>
                        ))}
                    </TabsList>

                    <GroupMenu label="Analyze" tabs={analyzeTabs} activeValue={activeTab} onSelect={setActiveTab} />
                    <GroupMenu label="Compliance" tabs={complianceTabs} activeValue={activeTab} onSelect={setActiveTab} />
                </div>

                <TabsContent value="search" className="mt-5">
                    <MultiSearch projectId={projectId} namespace={rawNamespace} dim={dim} onDelete={deleteRecord} />
                </TabsContent>
                <TabsContent value="upload" className="mt-5">
                    <DocumentUploadTab
                        projectId={projectId}
                        namespace={rawNamespace}
                        onAskQuestion={handleAskQuestion}
                        capabilities={capabilities}
                        settingsHref={settingsHref}
                    />
                </TabsContent>
                <TabsContent value="bulk" className="mt-5">
                    <BulkInsertTab projectId={projectId} namespace={rawNamespace} dim={dim} />
                </TabsContent>
                <TabsContent value="visualize" className="mt-5">
                    <VisualizeTab projectId={projectId} namespace={rawNamespace} dim={dim} />
                </TabsContent>
                <TabsContent value="ask" className="mt-5">
                    <AskTab projectId={projectId} namespace={rawNamespace} initialQuestion={pendingQuestion} />
                </TabsContent>
                <TabsContent value="docs" className="mt-5">
                    <DocumentsTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="index" className="mt-5">
                    <IndexLifecycleTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="treerag" className="mt-5">
                    <TreeRagTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="community" className="mt-5">
                    <CommunityTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="entities" className="mt-5">
                    <EntityExtractionTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="eval" className="mt-5">
                    <EvalTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="diff" className="mt-5">
                    <DiffTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="contradict" className="mt-5">
                    <ContradictionTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="verify" className="mt-5">
                    <VerifyTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="certify" className="mt-5">
                    <CertifyTab projectId={projectId} namespace={rawNamespace} collection={namespace} />
                </TabsContent>
                <TabsContent value="gdpr" className="mt-5">
                    <GdprTab projectId={projectId} namespace={rawNamespace} />
                </TabsContent>
                <TabsContent value="compliance" className="mt-5">
                    <CompliancePackTab projectId={projectId} namespace={rawNamespace} collection={namespace} />
                </TabsContent>
                <TabsContent value="info" className="mt-5">
                    <CollectionInfo projectName={projectName} namespace={rawNamespace} dim={dim} />
                </TabsContent>
                {extraTabs?.map(({ value, render }) => (
                    <TabsContent key={value} value={value} className="mt-5">
                        {render()}
                    </TabsContent>
                ))}
            </Tabs>
        </div>
    )
}
