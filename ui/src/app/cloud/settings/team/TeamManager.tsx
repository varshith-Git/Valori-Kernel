'use client'

import { useState, useTransition } from 'react'
import { useRouter } from 'next/navigation'
import { Button } from '@/components/ui/button'
import { StatusBadge } from '@/components/ui/status-badge'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { inviteMember, revokeInvitation, changeMemberRole, removeMember, transferOwnership } from './actions'

interface Member {
    member_user_id: string
    email: string
    role: string
    joined_at: string
}

interface Invitation {
    id: string
    email: string
    role: string
    created_at: string
    expires_at: string
}

const ROLES = ['owner', 'admin', 'developer', 'viewer']

function fmtDate(iso: string) {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
}

export function TeamManager({
    orgId,
    orgName,
    myUserId,
    canManage,
    canChangeRoles,
    initialMembers,
    initialInvitations,
}: {
    orgId: string
    orgName: string
    myUserId: string
    myRole: string
    canManage: boolean
    canChangeRoles: boolean
    initialMembers: Member[]
    initialInvitations: Invitation[]
}) {
    const router = useRouter()
    const [inviteEmail, setInviteEmail] = useState('')
    const [inviteRole, setInviteRole] = useState('developer')
    const [error, setError] = useState<string | null>(null)
    const [notice, setNotice] = useState<string | null>(null)
    const [isPending, startTransition] = useTransition()
    const [transferTarget, setTransferTarget] = useState<Member | null>(null)

    const handleInvite = () => {
        setError(null)
        setNotice(null)
        startTransition(async () => {
            const result = await inviteMember(orgId, orgName, inviteEmail, inviteRole)
            if (result.error) {
                setError(result.error)
                return
            }
            if (result.warning) setNotice(result.warning)
            setInviteEmail('')
            router.refresh()
        })
    }

    const handleRevoke = (id: string) => {
        startTransition(async () => {
            const result = await revokeInvitation(id)
            if (result.error) setError(result.error)
            else router.refresh()
        })
    }

    const handleRoleChange = (userId: string, role: string) => {
        startTransition(async () => {
            const result = await changeMemberRole(orgId, userId, role)
            if (result.error) setError(result.error)
            else router.refresh()
        })
    }

    const handleRemove = (userId: string) => {
        startTransition(async () => {
            const result = await removeMember(orgId, userId)
            if (result.error) setError(result.error)
            else router.refresh()
        })
    }

    const handleTransfer = () => {
        if (!transferTarget) return
        setError(null)
        startTransition(async () => {
            const result = await transferOwnership(orgId, transferTarget.member_user_id)
            if (result.error) {
                setError(result.error)
                return
            }
            setTransferTarget(null)
            router.refresh()
        })
    }

    return (
        <div className="space-y-6">
            {error && (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                    <p className="text-sm text-destructive">{error}</p>
                </div>
            )}
            {notice && (
                <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3">
                    <p className="text-sm text-amber-600">{notice}</p>
                </div>
            )}

            {/* Members */}
            <div className="rounded-xl border border-border bg-card overflow-hidden">
                <table className="w-full text-sm">
                    <thead>
                        <tr className="border-b border-border text-left text-xs text-muted-foreground uppercase tracking-widest">
                            <th className="px-6 py-3 font-medium">Email</th>
                            <th className="px-6 py-3 font-medium">Role</th>
                            <th className="px-6 py-3 font-medium">Joined</th>
                            {canManage && <th className="px-6 py-3 font-medium" />}
                        </tr>
                    </thead>
                    <tbody>
                        {initialMembers.map((m) => {
                            const isSelf = m.member_user_id === myUserId
                            return (
                                <tr key={m.member_user_id} className="border-b border-border last:border-0">
                                    <td className="px-6 py-4 text-foreground font-medium">
                                        {m.email}
                                        {isSelf && <span className="ml-2 text-xs text-muted-foreground">(you)</span>}
                                    </td>
                                    <td className="px-6 py-4">
                                        {canChangeRoles && !isSelf ? (
                                            <select
                                                value={m.role}
                                                onChange={(e) => handleRoleChange(m.member_user_id, e.target.value)}
                                                disabled={isPending}
                                                className="rounded-md border border-input bg-background px-2 py-1 text-xs text-foreground"
                                            >
                                                {ROLES.map((r) => (
                                                    <option key={r} value={r}>{r}</option>
                                                ))}
                                            </select>
                                        ) : (
                                            <StatusBadge tone="neutral">{m.role}</StatusBadge>
                                        )}
                                    </td>
                                    <td className="px-6 py-4 text-muted-foreground">{fmtDate(m.joined_at)}</td>
                                    {canManage && (
                                        <td className="px-6 py-4 text-right space-x-3">
                                            {canChangeRoles && !isSelf && m.role !== 'owner' && (
                                                <button
                                                    onClick={() => setTransferTarget(m)}
                                                    disabled={isPending}
                                                    className="text-xs text-muted-foreground hover:text-foreground hover:underline disabled:opacity-50"
                                                >
                                                    Make owner
                                                </button>
                                            )}
                                            {!isSelf && (
                                                <button
                                                    onClick={() => handleRemove(m.member_user_id)}
                                                    disabled={isPending}
                                                    className="text-xs text-destructive hover:underline disabled:opacity-50"
                                                >
                                                    Remove
                                                </button>
                                            )}
                                        </td>
                                    )}
                                </tr>
                            )
                        })}
                    </tbody>
                </table>
            </div>

            {/* Invite */}
            {canManage && (
                <div className="rounded-xl border border-border bg-card p-5 space-y-3">
                    <p className="text-xs text-muted-foreground uppercase tracking-widest">Invite someone</p>
                    <div className="flex flex-wrap gap-2">
                        <input
                            type="email"
                            value={inviteEmail}
                            onChange={(e) => setInviteEmail(e.target.value)}
                            placeholder="teammate@example.com"
                            className="flex-1 min-w-[200px] rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
                        />
                        <select
                            value={inviteRole}
                            onChange={(e) => setInviteRole(e.target.value)}
                            className="rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground"
                        >
                            {ROLES.filter((r) => r !== 'owner').map((r) => (
                                <option key={r} value={r}>{r}</option>
                            ))}
                        </select>
                        <Button size="sm" onClick={handleInvite} disabled={isPending || !inviteEmail.trim()}>
                            {isPending ? 'Sending…' : 'Send Invite'}
                        </Button>
                    </div>
                </div>
            )}

            {/* Pending invitations */}
            {canManage && initialInvitations.length > 0 && (
                <div className="rounded-xl border border-border bg-card overflow-hidden">
                    <div className="px-6 py-3 border-b border-border">
                        <p className="text-xs text-muted-foreground uppercase tracking-widest">Pending invitations</p>
                    </div>
                    <table className="w-full text-sm">
                        <tbody>
                            {initialInvitations.map((inv) => (
                                <tr key={inv.id} className="border-b border-border last:border-0">
                                    <td className="px-6 py-4 text-foreground">{inv.email}</td>
                                    <td className="px-6 py-4">
                                        <StatusBadge tone="neutral">{inv.role}</StatusBadge>
                                    </td>
                                    <td className="px-6 py-4 text-muted-foreground text-xs">
                                        Expires {fmtDate(inv.expires_at)}
                                    </td>
                                    <td className="px-6 py-4 text-right">
                                        <button
                                            onClick={() => handleRevoke(inv.id)}
                                            disabled={isPending}
                                            className="text-xs text-destructive hover:underline disabled:opacity-50"
                                        >
                                            Revoke
                                        </button>
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                </div>
            )}

            <Dialog open={transferTarget !== null} onOpenChange={(o) => { if (!o) setTransferTarget(null) }}>
                <DialogContent className="bg-card border-input max-w-sm">
                    <DialogHeader>
                        <DialogTitle className="text-foreground text-base">Transfer ownership</DialogTitle>
                    </DialogHeader>
                    <div className="flex flex-col gap-3 pt-1">
                        <p className="text-sm text-muted-foreground">
                            Make <strong className="text-foreground">{transferTarget?.email}</strong> the owner of{' '}
                            <strong className="text-foreground">{orgName}</strong>? You&apos;ll be demoted to admin.
                        </p>
                        <div className="flex gap-2 justify-end">
                            <Button variant="ghost" size="sm" onClick={() => setTransferTarget(null)}>
                                Cancel
                            </Button>
                            <Button size="sm" onClick={handleTransfer} disabled={isPending}>
                                {isPending ? 'Transferring…' : 'Transfer ownership'}
                            </Button>
                        </div>
                    </div>
                </DialogContent>
            </Dialog>
        </div>
    )
}
