import { createClient } from '@/utils/supabase/server'
import { redirect } from 'next/navigation'
import { SecurityManager } from './SecurityManager'
import { IpAllowlistManager } from './IpAllowlistManager'
import { SettingsNav } from '../SettingsNav'

export default async function SecurityPage() {
    const supabase = await createClient()

    const {
        data: { user },
    } = await supabase.auth.getUser()

    if (!user) {
        redirect('/login')
    }

    // Same "first org" simplification as elsewhere — no org switcher yet.
    const { data: memberships } = await supabase
        .from('org_members')
        .select('role, organizations(id, name)')
        .eq('user_id', user.id)
        .limit(1)
    const membership = memberships?.[0] as { role: string; organizations: { id: string; name: string } } | undefined
    const org = membership?.organizations
    const canManageOrg = membership?.role === 'owner' || membership?.role === 'admin'

    const [{ data: factorsData }, { data: sessions }, { data: loginHistory }, { data: ipRules }] = await Promise.all([
        supabase.auth.mfa.listFactors(),
        supabase.rpc('list_my_sessions'),
        supabase.from('login_history').select('*').order('created_at', { ascending: false }).limit(20),
        org
            ? supabase.from('ip_allowlist_rules').select('*').eq('org_id', org.id).order('created_at', { ascending: false })
            : Promise.resolve({ data: [] }),
    ])

    return (
        <div className="min-h-screen p-4 sm:p-8">
            <div className="w-full max-w-3xl mx-auto space-y-6">
                <div>
                    <h1 className="text-2xl font-bold tracking-tight text-foreground">Settings</h1>
                    <p className="text-sm text-muted-foreground mt-1">
                        Two-factor authentication and active sessions for {user.email}.
                    </p>
                </div>

                <SettingsNav />

                <SecurityManager
                    initialFactors={factorsData?.totp ?? []}
                    initialSessions={(sessions ?? []) as SessionRow[]}
                    initialLoginHistory={(loginHistory ?? []) as LoginHistoryRow[]}
                />

                {org && (
                    <IpAllowlistManager
                        orgId={org.id}
                        orgName={org.name}
                        canManage={canManageOrg}
                        initialRules={(ipRules ?? []) as IpAllowlistRuleRow[]}
                    />
                )}
            </div>
        </div>
    )
}

export type SessionRow = {
    session_id: string
    created_at: string
    updated_at: string
    user_agent: string | null
    ip: string | null
    is_current: boolean
}

export type LoginHistoryRow = {
    id: string
    email: string
    success: boolean
    ip: string | null
    user_agent: string | null
    created_at: string
}

export type IpAllowlistRuleRow = {
    id: string
    org_id: string
    cidr: string
    description: string | null
    created_at: string
}
