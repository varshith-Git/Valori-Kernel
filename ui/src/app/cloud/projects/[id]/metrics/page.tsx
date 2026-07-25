import { createClient } from '@/utils/supabase/server'
import { redirect, notFound } from 'next/navigation'
import Link from 'next/link'
import { MetricsView } from '@/components/projects/MetricsView'

export default async function ProjectMetricsPage({ params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const supabase = await createClient()

    const {
        data: { user },
    } = await supabase.auth.getUser()

    if (!user) {
        redirect('/login')
    }

    const { data: project } = await supabase.from('projects').select('*').eq('id', id).single()

    if (!project) {
        notFound()
    }

    return (
        <div className="min-h-screen p-4 sm:p-8">
            <div className="w-full max-w-6xl mx-auto space-y-6">
                <div>
                    <Link href={`/cloud/projects/${id}`} className="text-xs text-muted-foreground hover:text-foreground">
                        ← {project.name}
                    </Link>
                    <h1 className="text-2xl font-bold tracking-tight text-foreground mt-2">Metrics</h1>
                </div>


                {project.status !== 'active' ? (
                    <div className="rounded-xl border border-border bg-card p-8 text-center">
                        <p className="text-sm text-muted-foreground">Project is {project.status}.</p>
                    </div>
                ) : (
                    <MetricsView projectId={project.id} />
                )}
            </div>
        </div>
    )
}
