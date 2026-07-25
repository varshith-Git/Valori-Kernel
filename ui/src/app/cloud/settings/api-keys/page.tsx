import { createClient } from '@/utils/supabase/server'
import { redirect } from 'next/navigation'
import { ApiKeysManager } from './ApiKeysManager'
import { ServiceAccountsManager } from './ServiceAccountsManager'
import { SettingsNav } from '../SettingsNav'

export default async function ApiKeysPage() {
    const supabase = await createClient()

    const {
        data: { user },
    } = await supabase.auth.getUser()

    if (!user) {
        redirect('/login')
    }

    // Same "first org" simplification as /cloud — no org switcher yet.
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
    const canManage = membership.role === 'owner' || membership.role === 'admin'

    const [{ data: keys }, { data: subscription }, { data: serviceAccounts }] = await Promise.all([
        supabase.from('api_keys_public').select('*').eq('org_id', org.id).order('created_at', { ascending: false }),
        supabase.from('subscriptions').select('plans(rate_limit_per_minute)').eq('org_id', org.id).single(),
        supabase.from('service_accounts').select('*').eq('org_id', org.id).order('created_at', { ascending: false }),
    ])

    const rateLimitPerMinute = (subscription?.plans as unknown as { rate_limit_per_minute: number } | null)?.rate_limit_per_minute ?? null

    return (
        <div className="min-h-screen p-4 sm:p-8">
            <div className="w-full max-w-3xl mx-auto space-y-6">
                <div>
                    <h1 className="text-2xl font-bold tracking-tight text-foreground">Settings</h1>
                    <p className="text-sm text-muted-foreground mt-1">
                        {org.name} — keys for programmatic access to your projects.
                        {rateLimitPerMinute && ` Rate limit: ${rateLimitPerMinute.toLocaleString()} requests/minute per key.`}
                    </p>
                </div>

                <SettingsNav />

                <ServiceAccountsManager orgId={org.id} canManage={canManage} initialAccounts={serviceAccounts ?? []} />

                <ApiKeysManager
                    orgId={org.id}
                    canManage={canManage}
                    initialKeys={keys ?? []}
                    serviceAccounts={(serviceAccounts ?? []).filter((a) => !a.disabled_at)}
                />
            </div>
        </div>
    )
}

export type ServiceAccountRow = {
    id: string
    org_id: string
    name: string
    description: string | null
    created_at: string
    disabled_at: string | null
}
