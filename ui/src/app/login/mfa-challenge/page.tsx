import { ShieldCheck } from 'lucide-react'
import { verifyMfaChallenge } from './actions'

export default async function MfaChallengePage({
    searchParams,
}: {
    searchParams: Promise<{ next?: string; error?: string; desktop?: string }>
}) {
    const { next, error, desktop } = await searchParams

    return (
        <div className="min-h-screen flex items-center justify-center bg-background text-foreground p-4">
            <div className="w-full max-w-sm space-y-6">
                <div className="text-center space-y-2">
                    <ShieldCheck className="mx-auto text-primary" size={32} />
                    <h1 className="text-xl font-bold tracking-tight font-mono">TWO_FACTOR_REQUIRED</h1>
                    <p className="text-sm text-muted-foreground">
                        Enter the 6-digit code from your authenticator app.
                    </p>
                </div>

                {error && <p className="text-xs text-destructive text-center">{error}</p>}

                <form action={verifyMfaChallenge} className="space-y-4">
                    <input type="hidden" name="next" value={next ?? '/dashboard'} />
                    {desktop === '1' && <input type="hidden" name="desktop" value="1" />}
                    <input
                        name="code"
                        inputMode="numeric"
                        autoComplete="one-time-code"
                        maxLength={6}
                        required
                        autoFocus
                        placeholder="000000"
                        className="w-full text-center tracking-[0.5em] text-lg rounded-md border border-input bg-background px-3 py-3 font-mono focus:outline-none focus:ring-1 focus:ring-ring"
                    />
                    <button
                        type="submit"
                        className="w-full rounded-md bg-primary text-primary-foreground py-2.5 text-sm font-medium hover:opacity-90 transition"
                    >
                        Verify
                    </button>
                </form>
            </div>
        </div>
    )
}
