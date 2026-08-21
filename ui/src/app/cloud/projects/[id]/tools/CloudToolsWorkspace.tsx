'use client'

// Thin host wrapper around @valori/studio's ToolsWorkspace: owns the
// `?collection=` URL sync and post-mutation refresh that the old Cloud
// ToolsWorkspace did internally with `next/navigation` — Shared Studio
// itself can't import next/navigation, so this is exactly the callback
// seam Phase C built for that (onCollectionChange / onMutate), same
// pattern as Desktop Local's migrated routes.

import { useCallback } from 'react'
import { useRouter } from 'next/navigation'
import { ToolsWorkspace } from '@valori/studio'
import { CloudStudioProvider } from '@/lib/cloud-runtime/CloudStudioProvider'

export function CloudToolsWorkspace({
    projectId,
    projectName,
    initialCollection,
}: {
    projectId: string
    projectName: string
    initialCollection?: string
}) {
    const router = useRouter()

    const onCollectionChange = useCallback(
        (name: string) => {
            router.replace(`/cloud/projects/${projectId}/tools?collection=${encodeURIComponent(name)}`, { scroll: false })
        },
        [projectId, router]
    )

    const onMutate = useCallback(() => {
        router.refresh()
    }, [router])

    return (
        <CloudStudioProvider>
            <ToolsWorkspace
                projectId={projectId}
                projectName={projectName}
                initialCollection={initialCollection}
                onCollectionChange={onCollectionChange}
                onMutate={onMutate}
            />
        </CloudStudioProvider>
    )
}
