'use client'

import { useEffect, useState } from 'react'
import { useRouter, useSearchParams } from 'next/navigation'
import { createClient } from '@/utils/supabase/client'
import { Loader2, ShieldCheck, AlertTriangle } from 'lucide-react'
import { Suspense } from 'react'

// The desktop shell's Rust side (on_open_url in src-tauri/src/lib.rs)
// navigates the embedded webview here after receiving a
// valori://auth-callback?access_token=...&refresh_token=... deep link —
// see that website's /desktop-handoff page for where those tokens come
// from. This is the ONLY page in the app that ever sees them; it hands
// them to Supabase's own session store immediately and never persists
// them itself.
function DesktopReceivedInner() {
    const router = useRouter()
    const params = useSearchParams()
    const [error, setError] = useState<string | null>(null)
    const [success, setSuccess] = useState<boolean>(false)

    useEffect(() => {
        const accessToken = params.get('access_token')
        const refreshToken = params.get('refresh_token')

        if (!accessToken || !refreshToken) {
            setError('Missing tokens in the sign-in handoff — try "Sign in to sync" again.')
            return
        }

        const run = async () => {
            const supabase = createClient()
            const { error: sessionError } = await supabase.auth.setSession({
                access_token: accessToken,
                refresh_token: refreshToken,
            })
            if (sessionError) {
                setError(sessionError.message)
                return
            }

            const res = await fetch('/api/mode', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ mode: 'cloud' }),
            })
            if (!res.ok) {
                setError('Signed in, but could not switch this app into cloud mode. Try restarting it.')
                return
            }

            setSuccess(true)
            setTimeout(() => {
                window.location.href = '/'
            }, 1500)
        }
        run()
    }, [params, router])

    if (error) {
        return (
            <div className="min-h-screen flex flex-col items-center justify-center gap-3 p-4 text-center">
                <AlertTriangle className="text-destructive" size={28} />
                <p className="text-sm text-muted-foreground max-w-sm">{error}</p>
            </div>
        )
    }

    if (success) {
        return (
            <div className="min-h-screen flex flex-col items-center justify-center gap-3 text-center">
                <ShieldCheck className="text-primary animate-bounce" size={32} />
                <h1 className="text-lg font-semibold font-mono">SIGN_IN_SUCCESSFUL</h1>
                <p className="text-sm text-muted-foreground">Welcome to Valori! Redirecting to home page…</p>
            </div>
        )
    }

    return (
        <div className="min-h-screen flex flex-col items-center justify-center gap-3">
            <div className="relative">
                <Loader2 className="animate-spin text-muted-foreground" size={28} />
                <ShieldCheck className="absolute inset-0 m-auto text-primary" size={14} />
            </div>
            <p className="text-sm text-muted-foreground">Finishing sign-in…</p>
        </div>
    )
}

export default function DesktopReceivedPage() {
    return (
        <Suspense fallback={null}>
            <DesktopReceivedInner />
        </Suspense>
    )
}
