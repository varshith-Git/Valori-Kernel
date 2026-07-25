'use client'

import { Suspense } from 'react'
import { useSearchParams } from 'next/navigation'
import { KeyRound } from 'lucide-react'
import { updatePassword } from '../login/actions'

function ResetPasswordForm() {
    const error = useSearchParams().get('error')

    return (
        <div className="min-h-screen flex items-center justify-center bg-background text-foreground p-4">
            <div className="w-full max-w-md space-y-8">
                <div className="text-center">
                    <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-primary/10 text-primary text-xs font-mono font-bold tracking-wider mb-6 border border-primary/20">
                        <KeyRound size={12} />
                        SET_NEW_PASSWORD
                    </div>
                    <h2 className="text-2xl font-bold tracking-tight font-mono">CHOOSE_A_NEW_PASSWORD</h2>
                    <p className="mt-2 text-sm text-muted-foreground">
                        At least 8 characters.
                    </p>
                </div>

                {error && (
                    <div className="rounded-md border border-destructive/30 bg-destructive/10 px-4 py-3">
                        <p className="text-sm text-destructive">{error}</p>
                    </div>
                )}

                <form className="space-y-4">
                    <div>
                        <label htmlFor="password" className="sr-only">New password</label>
                        <input
                            id="password"
                            name="password"
                            type="password"
                            autoComplete="new-password"
                            required
                            minLength={8}
                            className="relative block w-full rounded-md border border-input bg-background px-3 py-3 text-foreground placeholder:text-muted-foreground focus:z-10 focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary sm:text-sm font-mono"
                            placeholder="New password"
                        />
                    </div>
                    <div>
                        <label htmlFor="confirm" className="sr-only">Confirm new password</label>
                        <input
                            id="confirm"
                            name="confirm"
                            type="password"
                            autoComplete="new-password"
                            required
                            minLength={8}
                            className="relative block w-full rounded-md border border-input bg-background px-3 py-3 text-foreground placeholder:text-muted-foreground focus:z-10 focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary sm:text-sm font-mono"
                            placeholder="Confirm new password"
                        />
                    </div>
                    <button
                        formAction={updatePassword}
                        className="group relative flex w-full justify-center items-center gap-2 rounded-none bg-primary text-black py-3 px-4 text-sm font-bold tracking-wider hover:bg-primary/90 focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2 transition-all font-mono uppercase"
                    >
                        <KeyRound size={16} />
                        Update password
                    </button>
                </form>
            </div>
        </div>
    )
}

export default function ResetPasswordPage() {
    return (
        <Suspense fallback={<div className="min-h-screen flex items-center justify-center font-mono text-sm">LOADING...</div>}>
            <ResetPasswordForm />
        </Suspense>
    )
}
