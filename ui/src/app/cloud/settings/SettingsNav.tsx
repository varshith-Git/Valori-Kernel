'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { cn } from '@/lib/utils'
import { KeyRound, ShieldCheck, Users, ScrollText, TerminalSquare } from 'lucide-react'

const TABS = [
    { href: '/cloud/settings/api-keys', label: 'API Keys', Icon: KeyRound },
    { href: '/cloud/settings/developer', label: 'Developer', Icon: TerminalSquare },
    { href: '/cloud/settings/security', label: 'Security', Icon: ShieldCheck },
    { href: '/cloud/settings/team', label: 'Team', Icon: Users },
    { href: '/cloud/archived', label: 'Archived Projects', Icon: ScrollText },
]

export function SettingsNav() {
    const path = usePathname()

    return (
        <nav className="flex items-center gap-1 border-b border-border overflow-x-auto" aria-label="Settings sections">
            {TABS.map(({ href, label, Icon }) => {
                const active = path === href
                return (
                    <Link
                        key={href}
                        href={href}
                        className={cn(
                            'flex items-center gap-1.5 whitespace-nowrap px-3 py-2.5 text-sm font-medium border-b-2 -mb-px transition-colors',
                            active
                                ? 'border-[var(--v-accent)] text-foreground'
                                : 'border-transparent text-muted-foreground hover:text-foreground hover:border-border'
                        )}
                    >
                        <Icon size={13} aria-hidden />
                        {label}
                    </Link>
                )
            })}
        </nav>
    )
}
