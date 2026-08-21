import { createClient } from '@/utils/supabase/server'
import { getAuthedUser, getCurrentMembership } from '@/utils/supabase/dal'
import { redirect } from 'next/navigation'
import { TeamManager } from './TeamManager'
import { SettingsNav } from '../SettingsNav'

export default async function TeamPage() {
    const supabase = await createClient()
    const user = await getAuthedUser()

    if (!user) {
        redirect('/login')
    }

    const membership = await getCurrentMembership()

    if (!membership) {
        return (
            <div className="min-h-screen flex items-center justify-center p-4">
                <p className="text-muted-foreground">No organization found for this account.</p>
            </div>
        )
    }

    const org = membership.organizations
    const myRole = membership.role
    const canManage = myRole === 'owner' || myRole === 'admin'
    const canChangeRoles = myRole === 'owner'

    const { data: members } = await supabase.rpc('list_org_members', { target_org_id: org.id })

    const { data: invitations } = canManage
        ? await supabase
            .from('org_invitations')
            .select('*')
            .eq('org_id', org.id)
            .is('accepted_at', null)
            .order('created_at', { ascending: false })
        : { data: [] }

    return (
        <div className="min-h-screen p-4 sm:p-8">
            <div className="w-full max-w-3xl mx-auto space-y-6">
                <div>
                    <h1 className="text-2xl font-bold tracking-tight text-foreground">Settings</h1>
                    <p className="text-sm text-muted-foreground mt-1">
                        {org.name} — members and pending invitations.
                    </p>
                </div>

                <SettingsNav />

                <TeamManager
                    orgId={org.id}
                    orgName={org.name}
                    myUserId={user.id}
                    myRole={myRole}
                    canManage={canManage}
                    canChangeRoles={canChangeRoles}
                    initialMembers={members ?? []}
                    initialInvitations={invitations ?? []}
                />
            </div>
        </div>
    )
}
