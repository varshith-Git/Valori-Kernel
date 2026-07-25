import { createClient } from '@/utils/supabase/server'
import { redirect } from 'next/navigation'
import { ArchivedProjects } from './ArchivedProjects'
import { SettingsNav } from '../settings/SettingsNav'

export default async function ArchivedPage() {
    const supabase = await createClient()

    const {
        data: { user },
    } = await supabase.auth.getUser()

    if (!user) {
        redirect('/login')
    }

    const { data: memberships } = await supabase
        .from('org_members')
        .select('role, organizations(id, name)')
        .eq('user_id', user.id)
        .limit(1)

    const membership = memberships?.[0] as
        | { role: string; organizations: { id: string; name: string } }
        | undefined

    if (!membership) {
        return (
            <div className="min-h-screen flex items-center justify-center p-4">
                <p className="text-muted-foreground">No organization found for this account.</p>
            </div>
        )
    }

    const org = membership.organizations

    const { data: projects } = await supabase
        .from('projects')
        .select('id, name, region, updated_at')
        .eq('org_id', org.id)
        .eq('status', 'archived')
        .order('updated_at', { ascending: false })

    return (
        <div className="min-h-screen p-4 sm:p-8">
            <div className="w-full max-w-3xl mx-auto space-y-6">
                <div>
                    <h1 className="text-2xl font-bold tracking-tight text-foreground">Settings</h1>
                    <p className="text-sm text-muted-foreground mt-1">
                        {org.name} — stopped and hidden from the main list. Restore to bring one back.
                    </p>
                </div>

                <SettingsNav />

                <ArchivedProjects initialProjects={projects ?? []} />
            </div>
        </div>
    )
}
