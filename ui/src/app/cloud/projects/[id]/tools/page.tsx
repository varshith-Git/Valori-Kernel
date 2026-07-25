import { createClient } from '@/utils/supabase/server'
import { redirect, notFound } from 'next/navigation'
import { ToolsWorkspace } from './ToolsWorkspace'

export default async function ProjectToolsPage({
    params,
    searchParams,
}: {
    params: Promise<{ id: string }>
    searchParams: Promise<{ collection?: string }>
}) {
    const { id } = await params
    const { collection } = await searchParams
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
            <div className="w-full max-w-[1600px] mx-auto space-y-5">
                {project.status !== 'active' ? (
                    <div className="rounded-xl border border-border bg-card p-8 text-center">
                        <p className="text-sm text-muted-foreground">Project is {project.status}.</p>
                    </div>
                ) : (
                    <ToolsWorkspace projectId={project.id} projectName={project.name} initialCollection={collection} />
                )}
            </div>
        </div>
    )
}
