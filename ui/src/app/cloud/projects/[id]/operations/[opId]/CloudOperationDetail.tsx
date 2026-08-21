'use client'

// Thin client wrapper — `renderExecution` is a function prop and the page
// itself is a Server Component, so this boundary needs a client component
// to hold the closure (Next.js can't serialize functions across the
// server/client boundary). Desktop Cloud already depends on @xyflow/react,
// so it can supply the same ExecutionExplorer Local uses.
//
// Phase L (performance): dynamic-imported (see the identical comment in
// Local's src/app/operations/[id]/page.tsx) so @xyflow/react only loads when
// the Execution Explorer tab is actually opened, not on every operation
// detail page view.

import dynamic from 'next/dynamic'
import { OperationDetailView } from '@valori/studio'
import { CloudStudioProvider } from '@/lib/cloud-runtime/CloudStudioProvider'

const ExecutionExplorer = dynamic(() => import('@/components/operations/ExecutionExplorer'), {
    ssr: false,
    loading: () => <div className="p-6 text-sm text-muted-foreground">Loading execution graph…</div>,
})

export function CloudOperationDetail({ projectId, operationId }: { projectId: string; operationId: string }) {
    return (
        <CloudStudioProvider>
            <OperationDetailView
                projectId={projectId}
                operationId={operationId}
                backHref={`/cloud/projects/${projectId}/operations`}
                renderExecution={(data, loading) => <ExecutionExplorer loading={loading} data={data} />}
            />
        </CloudStudioProvider>
    )
}
