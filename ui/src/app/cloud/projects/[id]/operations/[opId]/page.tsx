import { getAuthedUser, getCloudProject } from '@/utils/supabase/dal'
import { redirect, notFound } from 'next/navigation'
import { CloudOperationDetail } from './CloudOperationDetail'

export default async function ProjectOperationDetailPage({
    params,
}: {
    params: Promise<{ id: string; opId: string }>
}) {
    const { id, opId } = await params
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
            <div className="w-full max-w-6xl mx-auto">
                <CloudOperationDetail projectId={project.id} operationId={opId} />
            </div>
        </div>
    )
}
