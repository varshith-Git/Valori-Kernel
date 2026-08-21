import { getAuthedUser, getCloudProject } from '@/utils/supabase/dal'
import { redirect, notFound } from 'next/navigation'
import { ClusterView } from '@valori/studio'
import { CloudStudioProvider } from '@/lib/cloud-runtime/CloudStudioProvider'

export default async function ProjectClusterPage({ params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const user = await getAuthedUser()

    if (!user) {
        redirect('/login')
    }

    const project = await getCloudProject(id)

    if (!project) {
        notFound()
    }

    // Cloud-appropriate hint: replication is a project setting chosen at
    // creation, not an env var the customer can flip — this deliberately
    // does NOT reuse Local's VALORI_CLUSTER_MEMBERS/docker-compose text.
    const standaloneHint = (
        <p className="mt-2 text-xs text-muted-foreground max-w-sm mx-auto">
            This project is running as a single node. Multi-node replication is
            chosen when a project is created and can&apos;t be changed after —
            create a new project with more than one node to enable it.
        </p>
    )

    return (
        <div className="min-h-screen p-4 sm:p-8">
            <div className="w-full max-w-6xl mx-auto space-y-6">
                {project.status !== 'active' ? (
                    <div className="rounded-xl border border-border bg-card p-8 text-center">
                        <p className="text-sm text-muted-foreground">Project is {project.status}.</p>
                    </div>
                ) : (
                    <CloudStudioProvider>
                        <ClusterView projectId={project.id} standaloneHint={standaloneHint} />
                    </CloudStudioProvider>
                )}
            </div>
        </div>
    )
}
