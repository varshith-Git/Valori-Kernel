import { createClient } from '@/utils/supabase/server'
import { redirect } from 'next/navigation'
import Link from 'next/link'
import { CreateProjectDialog } from './CreateProjectDialog'

const STATUS_STYLE: Record<string, string> = {
    active: 'bg-primary/10 text-primary border-primary/30',
    creating: 'bg-amber-500/10 text-amber-500 border-amber-500/30',
    error: 'bg-destructive/10 text-destructive border-destructive/30',
    stopped: 'bg-muted text-muted-foreground border-border',
    suspended: 'bg-amber-500/10 text-amber-500 border-amber-500/30',
    deleted: 'bg-muted text-muted-foreground border-border',
}

export default async function DashboardPage() {
    const supabase = await createClient()

    const {
        data: { user },
    } = await supabase.auth.getUser()

    if (!user) {
        redirect('/login')
    }

    const name = user.user_metadata.full_name || user.user_metadata.name || user.email?.split('@')[0] || 'User'

    // Every user has at least a personal org (see supabase/migrations —
    // handle_new_user trigger). Org switching isn't built yet, so this
    // dashboard always shows the first org the user is a member of.
    const { data: memberships } = await supabase
        .from('org_members')
        .select('role, organizations(id, name, is_personal)')
        .eq('user_id', user.id)
        .limit(1)

    const membership = memberships?.[0] as
        | { role: string; organizations: { id: string; name: string; is_personal: boolean } }
        | undefined

    if (!membership) {
        // Should be unreachable — the signup trigger always creates one —
        // but a defunct/pre-trigger account could hit this.
        return (
            <div className="min-h-screen flex items-center justify-center p-4">
                <p className="text-muted-foreground">
                    No organization found for this account. If you signed up before 2026-07-21, contact support.
                </p>
            </div>
        )
    }

    const org = membership.organizations

    const apiUrl = process.env.VALORI_CLOUD_API_URL ?? 'http://localhost:8787'

    const [{ data: projects }, { data: subscription }, { count: activeProjectCount }, freeProjectLimit] = await Promise.all([
        supabase
            .from('projects')
            .select('*')
            .eq('org_id', org.id)
            .neq('status', 'deleted')
            .neq('status', 'archived')
            .order('created_at', { ascending: false }),
        supabase.from('subscriptions').select('*').eq('org_id', org.id).single(),
        // Matches the backend's own count in check_free_plan_limit (non-deleted,
        // includes archived) — used only to decide whether to gray out "New
        // Project" before the user hits the 409 the backend would return anyway.
        supabase
            .from('projects')
            .select('id', { count: 'exact', head: true })
            .eq('org_id', org.id)
            .neq('status', 'deleted'),
        // Fail open (treat as "no limit known") if the backend is unreachable —
        // the backend still enforces the real limit server-side regardless.
        fetch(`${apiUrl}/v1/settings/public`, { cache: 'no-store' })
            .then((r) => (r.ok ? r.json() : null))
            .then((s: { free_project_limit?: number } | null) => s?.free_project_limit ?? null)
            .catch(() => null),
    ])

    const isFreePlan = (subscription?.plan ?? 'free') === 'free'
    const atProjectLimit = isFreePlan && freeProjectLimit != null && (activeProjectCount ?? 0) >= freeProjectLimit

    return (
        <div className="min-h-screen p-4 sm:p-8 relative overflow-hidden">
            <div className="absolute top-0 -left-1/4 w-1/2 h-1/2 bg-primary/10 rounded-full blur-3xl pointer-events-none" />
            <div className="absolute bottom-0 -right-1/4 w-1/2 h-1/2 bg-emerald-500/10 rounded-full blur-3xl pointer-events-none" />

            <div className="w-full max-w-4xl mx-auto relative z-10 space-y-6">
                <div className="flex items-center justify-between flex-wrap gap-4">
                    <div>
                        <h1 className="text-2xl font-bold tracking-tight text-foreground">
                            Welcome back, {name}
                        </h1>
                        <p className="text-sm text-muted-foreground mt-1">
                            {org.name}
                            <span className="mx-2 text-border">·</span>
                            <span className="uppercase tracking-widest text-xs">{subscription?.plan ?? 'free'} plan</span>
                        </p>
                    </div>
                    {/* Archived/Security/Team/API Keys now live in the sidebar
                        (see components/layout/AppSidebar.tsx) — kept only the
                        one action that belongs on this specific page. */}
                    <CreateProjectDialog orgId={org.id} atLimit={atProjectLimit} />
                </div>

                <div className="rounded-2xl border border-border bg-card/50 backdrop-blur-xl overflow-hidden">
                    {!projects || projects.length === 0 ? (
                        <div className="p-12 text-center">
                            <p className="text-muted-foreground text-sm">No projects yet.</p>
                        </div>
                    ) : (
                        <table className="w-full text-sm">
                            <thead>
                                <tr className="border-b border-border text-left text-xs text-muted-foreground uppercase tracking-widest">
                                    <th className="px-6 py-3 font-medium">Name</th>
                                    <th className="px-6 py-3 font-medium">Region</th>
                                    <th className="px-6 py-3 font-medium">Vector config</th>
                                    <th className="px-6 py-3 font-medium">Status</th>
                                    <th className="px-6 py-3 font-medium">Node URL</th>
                                </tr>
                            </thead>
                            <tbody>
                                {projects.map((p) => (
                                    <tr key={p.id} className="border-b border-border last:border-0 hover:bg-accent/30 transition-colors">
                                        <td className="px-6 py-4 text-foreground font-medium">
                                            <Link href={`/cloud/projects/${p.id}`} className="hover:underline">
                                                {p.name}
                                            </Link>
                                        </td>
                                        <td className="px-6 py-4 text-muted-foreground">{p.region}</td>
                                        <td className="px-6 py-4 font-mono text-xs text-muted-foreground">
                                            {p.dim}d · {p.index_type}
                                        </td>
                                        <td className="px-6 py-4">
                                            <span
                                                className={`inline-block px-2 py-0.5 rounded-full text-xs border ${STATUS_STYLE[p.status] ?? STATUS_STYLE.stopped}`}
                                            >
                                                {p.status}
                                            </span>
                                        </td>
                                        <td className="px-6 py-4 font-mono text-xs text-muted-foreground">
                                            {p.node_url ?? '—'}
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    )}
                </div>
            </div>
        </div>
    )
}
