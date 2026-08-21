import { getAuthedUser, getCloudProject } from '@/utils/supabase/dal'
import { redirect, notFound } from 'next/navigation'
import { SnapshotsView } from '@valori/studio'
import { CloudStudioProvider } from '@/lib/cloud-runtime/CloudStudioProvider'

export default async function ProjectSnapshotsPage({ params }: { params: Promise<{ id: string }> }) {
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
                <p className="text-sm text-muted-foreground">
                    Point-in-time captures of this project&apos;s state — save, download, or restore.
                </p>

                {project.status !== 'active' ? (
                    <div className="rounded-xl border border-border bg-card p-8 text-center">
                        <p className="text-sm text-muted-foreground">Project is {project.status}.</p>
                    </div>
                ) : (
                    <CloudStudioProvider>
                        <SnapshotsView projectId={project.id} />
                    </CloudStudioProvider>
                )}
            </div>
        </div>
    )
}
