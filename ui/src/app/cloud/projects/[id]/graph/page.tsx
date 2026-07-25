import { createClient } from '@/utils/supabase/server'
import { redirect, notFound } from 'next/navigation'
import { GraphView } from '@/components/graph/GraphView'

export default async function ProjectGraphPage({ params }: { params: Promise<{ id: string }> }) {
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
                <p className="text-sm text-muted-foreground">
                    Document/chunk knowledge graph for the default collection
                </p>

                {project.status !== 'active' ? (
                    <div className="rounded-xl border border-border bg-card p-8 text-center">
                        <p className="text-sm text-muted-foreground">Project is {project.status}.</p>
                    </div>
                ) : (
                    <GraphView projectId={project.id} namespace="default" />
                )}
            </div>
        </div>
    )
}
