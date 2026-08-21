import { getAuthedUser, getCloudProject } from '@/utils/supabase/dal'
import { redirect, notFound } from 'next/navigation'
import { MetricsView } from '@valori/studio'
import { CloudStudioProvider } from '@/lib/cloud-runtime/CloudStudioProvider'
import { resolveCloudCapabilities } from '@/lib/cloud-runtime/capabilities'

export default async function ProjectMetricsPage({ params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const user = await getAuthedUser()

    if (!user) {
        redirect('/login')
    }

    const project = await getCloudProject(id)

    if (!project) {
        notFound()
    }

    return (
        <div className="min-h-screen p-4 sm:p-8">
            <div className="w-full max-w-6xl mx-auto space-y-6">
                {project.status !== 'active' ? (
                    <div className="rounded-xl border border-border bg-card p-8 text-center">
                        <p className="text-sm text-muted-foreground">Project is {project.status}.</p>
                    </div>
                ) : (
                    <CloudStudioProvider>
                        <MetricsView projectId={project.id} capabilities={resolveCloudCapabilities()} />
                    </CloudStudioProvider>
                )}
            </div>
        </div>
    )
}
