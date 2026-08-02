import { createClient } from '@/utils/supabase/server'
import { redirect, notFound } from 'next/navigation'
import { OperationDetailView } from '@/components/operations/OperationDetailView'

export default async function ProjectOperationDetailPage({
    params,
}: {
    params: Promise<{ id: string; opId: string }>
}) {
    const { id, opId } = await params
    const supabase = await createClient()

    const {
        data: { user },
    } = await supabase.auth.getUser()

    if (!user) {
        redirect('/login')
    }

    const { data: project } = await supabase.from('projects').select('id, status').eq('id', id).single()

    if (!project) {
        notFound()
    }

    return (
        <div className="min-h-screen p-4 sm:p-8">
            <div className="w-full max-w-6xl mx-auto">
                <OperationDetailView projectId={project.id} operationId={opId} />
            </div>
        </div>
    )
}
