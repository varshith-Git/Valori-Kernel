'use client'

import { Suspense } from 'react'
import { useSearchParams } from 'next/navigation'
import Link from 'next/link'
import { Mail, Lock, CheckCircle2 } from 'lucide-react'
import { requestPasswordReset } from '../login/actions'

function ForgotPasswordForm() {
    const sent = useSearchParams().get('sent') === '1'

    return (
        <div className="min-h-screen flex items-center justify-center bg-background text-foreground p-4">
            <div className="w-full max-w-md space-y-8">
                <div className="text-center">
                    <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-primary/10 text-primary text-xs font-mono font-bold tracking-wider mb-6 border border-primary/20">
                        <Lock size={12} />
                        PASSWORD_RECOVERY
                    </div>
                    <h2 className="text-2xl font-bold tracking-tight font-mono">RESET_PASSWORD</h2>
                    <p className="mt-2 text-sm text-muted-foreground">
                        Enter your account email and we&apos;ll send a reset link.
                    </p>
                </div>

                {sent ? (
                    <div className="rounded-md border border-primary/20 bg-primary/5 p-6 text-center space-y-3">
                        <CheckCircle2 className="mx-auto text-primary" size={28} />
                        <p className="text-sm text-foreground">
                            If an account exists for that email, a reset link is on its way.
                        </p>
                        <p className="text-xs text-muted-foreground">
                            Check your inbox (and spam folder) — the link expires after a while, so use it soon.
                        </p>
                    </div>
                ) : (
                    <form className="space-y-4">
                        <div>
                            <label htmlFor="email" className="sr-only">Email address</label>
                            <input
                                id="email"
                                name="email"
                                type="email"
                                autoComplete="email"
                                required
                                className="relative block w-full rounded-md border border-input bg-background px-3 py-3 text-foreground placeholder:text-muted-foreground focus:z-10 focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary sm:text-sm font-mono"
                                placeholder="user@institution.com"
                            />
                        </div>
                        <button
                            formAction={requestPasswordReset}
                            className="group relative flex w-full justify-center items-center gap-2 rounded-none bg-primary text-black py-3 px-4 text-sm font-bold tracking-wider hover:bg-primary/90 focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2 transition-all font-mono uppercase"
                        >
                            <Mail size={16} />
                            Send reset link
                        </button>
                    </form>
                )}

                <p className="text-center text-xs text-muted-foreground">
                    <Link href="/login" className="hover:text-foreground transition-colors">
                        Back to login
                    </Link>
                </p>
            </div>
        </div>
    )
}

export default function ForgotPasswordPage() {
    return (
        <Suspense fallback={<div className="min-h-screen flex items-center justify-center font-mono text-sm">LOADING...</div>}>
            <ForgotPasswordForm />
        </Suspense>
    )
}
