import { getAuthedUser, getCurrentMembership } from '@/utils/supabase/dal'
import { redirect } from 'next/navigation'
import Link from 'next/link'
import { Cloud } from 'lucide-react'

// Reuses the same "first org" simplification as valori-ui's own dashboard
// (no org switcher yet — every account currently has exactly one org).
// Unlike valori-ui's dashboard/layout.tsx, this doesn't pull in AppSidebar —
// that component assumes it's the whole app's nav chrome, which would
// collide with this app's own local-mode Sidebar. This is a lightweight
// header instead; the desktop shell's global sidebar stays the primary nav.
export default async function CloudLayout({ children }: { children: React.ReactNode }) {
    const user = await getAuthedUser()

    if (!user) {
        redirect('/login')
    }

    const membership = await getCurrentMembership()
    const org = membership?.organizations

    return (
        <div className="min-h-screen bg-background text-foreground">
            <header className="flex items-center justify-between border-b border-border px-6 py-3">
                <Link href="/cloud" className="flex items-center gap-2 text-sm font-semibold text-foreground">
                    <Cloud size={16} className="text-[var(--v-accent)]" />
                    {org?.name ?? 'Personal'}
                </Link>
                <span className="text-xs text-muted-foreground">{user.email}</span>
            </header>
            <main>{children}</main>
        </div>
    )
}
