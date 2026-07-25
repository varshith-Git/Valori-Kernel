import { createClient } from '@/utils/supabase/server'
import { redirect, notFound } from 'next/navigation'
import Link from 'next/link'
import { ProjectWorkspace } from './ProjectWorkspace'
import { ProjectActions } from './ProjectActions'

export default async function ProjectPage({ params }: { params: Promise<{ id: string }> }) {
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
                <div className="flex items-start justify-between gap-4 flex-wrap">
                    <div>
                        <Link href="/cloud" className="text-xs text-muted-foreground hover:text-foreground">
                            ← Dashboard
                        </Link>
                        <h1 className="text-2xl font-bold tracking-tight text-foreground mt-2">{project.name}</h1>
                        <p className="text-sm text-muted-foreground mt-1 font-mono">
                            {project.region} · {project.replication === 1 ? 'single node' : `${project.replication}-node cluster`}
                        </p>
                    </div>
                    <ProjectActions
                        projectId={project.id}
                        orgId={project.org_id}
                        projectName={project.name}
                        projectStatus={project.status}
                    />
                </div>


                {project.status !== 'active' ? (
                    <div className="rounded-xl border border-border bg-card p-8 text-center">
                        <p className="text-sm text-muted-foreground">
                            {project.status === 'creating' && 'Provisioning — this project is still being deployed.'}
                            {project.status === 'error' && 'Provisioning failed. Try creating a new project.'}
                            {project.status === 'suspended' &&
                                'This project was automatically suspended after 30 days of inactivity on the Free plan. Hit Start to reactivate it — your data is untouched.'}
                            {(project.status === 'stopped' || project.status === 'deleted') && `Project is ${project.status}.`}
                        </p>
                    </div>
                ) : (
                    <ProjectWorkspace projectId={project.id} />
                )}
            </div>
        </div>
    )
}
