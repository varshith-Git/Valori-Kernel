'use client'

import { useState, useTransition } from 'react'
import { useRouter } from 'next/navigation'
import { Button } from '@/components/ui/button'
import { StatusBadge } from '@/components/ui/status-badge'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { enrollTotp, verifyEnrollment, unenrollFactor, signOutOtherSessions } from './actions'
import type { SessionRow, LoginHistoryRow } from './page'

interface Factor {
    id: string
    friendly_name?: string
    factor_type: string
    status: string
    created_at: string
}

function fmtDate(iso: string) {
    return new Date(iso).toLocaleString(undefined, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
    })
}

export function SecurityManager({
    initialFactors,
    initialSessions,
    initialLoginHistory,
}: {
    initialFactors: Factor[]
    initialSessions: SessionRow[]
    initialLoginHistory: LoginHistoryRow[]
}) {
    const router = useRouter()
    const [isPending, startTransition] = useTransition()
    const [error, setError] = useState<string | null>(null)

    // Enrollment flow state
    const [enrollOpen, setEnrollOpen] = useState(false)
    const [qrCode, setQrCode] = useState<string | null>(null)
    const [secret, setSecret] = useState<string | null>(null)
    const [factorId, setFactorId] = useState<string | null>(null)
    const [code, setCode] = useState('')

    const activeFactor = initialFactors.find((f) => f.status === 'verified')

    const startEnroll = () => {
        setError(null)
        startTransition(async () => {
            const result = await enrollTotp()
            if ('error' in result && result.error) {
                setError(result.error)
                return
            }
            setQrCode(result.qrCode ?? null)
            setSecret(result.secret ?? null)
            setFactorId(result.factorId ?? null)
            setEnrollOpen(true)
        })
    }

    const confirmEnroll = () => {
        if (!factorId) return
        setError(null)
        startTransition(async () => {
            const result = await verifyEnrollment(factorId, code.trim())
            if (result.error) {
                setError(result.error)
                return
            }
            setEnrollOpen(false)
            setQrCode(null)
            setSecret(null)
            setFactorId(null)
            setCode('')
            router.refresh()
        })
    }

    const disable = () => {
        if (!activeFactor) return
        setError(null)
        startTransition(async () => {
            const result = await unenrollFactor(activeFactor.id)
            if (result.error) {
                setError(result.error)
                return
            }
            router.refresh()
        })
    }

    const signOutOthers = () => {
        setError(null)
        startTransition(async () => {
            const result = await signOutOtherSessions()
            if (result.error) {
                setError(result.error)
                return
            }
            router.refresh()
        })
    }

    return (
        <div className="space-y-8">
            {error && (
                <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3">
                    <p className="text-sm text-destructive">{error}</p>
                </div>
            )}

            {/* MFA */}
            <section className="rounded-xl border border-border bg-card p-6 space-y-4">
                <div className="flex items-center justify-between">
                    <div>
                        <h2 className="text-sm font-semibold text-foreground">Two-Factor Authentication</h2>
                        <p className="text-xs text-muted-foreground mt-1">
                            Require a code from an authenticator app when signing in.
                        </p>
                    </div>
                    {activeFactor ? <StatusBadge tone="success">enabled</StatusBadge> : <StatusBadge tone="neutral">disabled</StatusBadge>}
                </div>

                {activeFactor ? (
                    <div className="flex items-center justify-between rounded-md border border-input bg-background px-4 py-3">
                        <span className="text-sm text-foreground">
                            {activeFactor.friendly_name || 'Authenticator app'} · added {fmtDate(activeFactor.created_at)}
                        </span>
                        <button
                            onClick={disable}
                            disabled={isPending}
                            className="text-xs text-destructive hover:underline disabled:opacity-50"
                        >
                            Disable
                        </button>
                    </div>
                ) : (
                    <Button size="sm" onClick={startEnroll} disabled={isPending}>
                        {isPending ? 'Starting…' : 'Enable 2FA'}
                    </Button>
                )}
            </section>

            {/* Sessions */}
            <section className="rounded-xl border border-border bg-card p-6 space-y-4">
                <div className="flex items-center justify-between">
                    <div>
                        <h2 className="text-sm font-semibold text-foreground">Active Sessions</h2>
                        <p className="text-xs text-muted-foreground mt-1">
                            Devices and browsers currently signed in to your account.
                        </p>
                    </div>
                    {initialSessions.length > 1 && (
                        <button
                            onClick={signOutOthers}
                            disabled={isPending}
                            className="text-xs text-destructive hover:underline disabled:opacity-50"
                        >
                            Sign out other sessions
                        </button>
                    )}
                </div>

                <div className="divide-y divide-border">
                    {initialSessions.map((s) => (
                        <div key={s.session_id} className="flex items-center justify-between py-3">
                            <div>
                                <p className="text-sm text-foreground">
                                    {s.user_agent ?? 'Unknown device'}
                                    {s.is_current && (
                                        <span className="ml-2">
                                            <StatusBadge tone="info">this device</StatusBadge>
                                        </span>
                                    )}
                                </p>
                                <p className="text-xs text-muted-foreground mt-0.5">
                                    {s.ip ? `${s.ip} · ` : ''}last active {fmtDate(s.updated_at)}
                                </p>
                            </div>
                        </div>
                    ))}
                    {initialSessions.length === 0 && (
                        <p className="text-sm text-muted-foreground py-3">No session data available.</p>
                    )}
                </div>
            </section>

            {/* Login history */}
            <section className="rounded-xl border border-border bg-card p-6 space-y-4">
                <div>
                    <h2 className="text-sm font-semibold text-foreground">Login History</h2>
                    <p className="text-xs text-muted-foreground mt-1">
                        Recent sign-in attempts against your account, successful or not.
                    </p>
                </div>
                <div className="divide-y divide-border">
                    {initialLoginHistory.map((h) => (
                        <div key={h.id} className="flex items-center justify-between py-3 gap-4">
                            <div>
                                <p className="text-sm text-foreground flex items-center gap-2">
                                    {h.success ? (
                                        <StatusBadge tone="success">success</StatusBadge>
                                    ) : (
                                        <StatusBadge tone="error">failed</StatusBadge>
                                    )}
                                    <span className="text-muted-foreground text-xs">{h.user_agent ?? 'Unknown device'}</span>
                                </p>
                                <p className="text-xs text-muted-foreground mt-0.5">
                                    {h.ip ? `${h.ip} · ` : ''}{fmtDate(h.created_at)}
                                </p>
                            </div>
                        </div>
                    ))}
                    {initialLoginHistory.length === 0 && (
                        <p className="text-sm text-muted-foreground py-3">No login history yet.</p>
                    )}
                </div>
            </section>

            {/* Enrollment dialog */}
            <Dialog open={enrollOpen} onOpenChange={(o) => { if (!o) setEnrollOpen(false) }}>
                <DialogContent className="bg-card border-input max-w-sm">
                    <DialogHeader>
                        <DialogTitle className="text-foreground text-base">Scan with your authenticator app</DialogTitle>
                    </DialogHeader>
                    <div className="flex flex-col gap-3 pt-1">
                        {qrCode && (
                            // eslint-disable-next-line @next/next/no-img-element
                            <img
                                src={qrCode}
                                alt="TOTP QR code"
                                className="mx-auto rounded-md border border-input bg-white p-2"
                                width={180}
                                height={180}
                            />
                        )}
                        {secret && (
                            <p className="text-[10px] text-muted-foreground text-center break-all font-mono">
                                Can&apos;t scan? Enter manually: {secret}
                            </p>
                        )}
                        <input
                            value={code}
                            onChange={(e) => setCode(e.target.value)}
                            onKeyDown={(e) => e.key === 'Enter' && confirmEnroll()}
                            inputMode="numeric"
                            maxLength={6}
                            placeholder="000000"
                            className="w-full text-center tracking-[0.4em] rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground font-mono focus:outline-none focus:ring-1 focus:ring-ring"
                        />
                        <div className="flex gap-2 justify-end">
                            <Button variant="ghost" size="sm" onClick={() => setEnrollOpen(false)}>
                                Cancel
                            </Button>
                            <Button size="sm" onClick={confirmEnroll} disabled={isPending || code.trim().length < 6}>
                                {isPending ? 'Verifying…' : 'Verify & enable'}
                            </Button>
                        </div>
                    </div>
                </DialogContent>
            </Dialog>
        </div>
    )
}
