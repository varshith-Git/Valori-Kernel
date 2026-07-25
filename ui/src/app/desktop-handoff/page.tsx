'use client'

import { Suspense, useEffect, useState } from 'react'
import { useSearchParams } from 'next/navigation'
import { ShieldCheck } from 'lucide-react'

// Landed on from /auth/callback/route.ts when the OAuth flow was started by
// the desktop app's "sign in to sync" (open_cloud_login in
// desktop/src-tauri/src/lib.rs) rather than a normal browser visit. Fires a
// valori://auth-callback deep link carrying the session — the desktop
// shell's on_open_url handler is registered for that scheme and forwards
// the tokens to its own embedded webview's /auth/desktop-received page.
// This tab is a fallback/confirmation, not the primary mechanism: most
// browsers act on custom-scheme navigation immediately without leaving
// this page, but nothing here depends on that succeeding silently.
function DesktopHandoffInner() {
    const params = useSearchParams()
    const [attempted, setAttempted] = useState(false)

    useEffect(() => {
        const accessToken = params.get('access_token')
        const refreshToken = params.get('refresh_token')
        if (!accessToken || !refreshToken) return

        // Strip tokens from the visible URL/history before navigating away —
        // defense in depth, not load-bearing (the deep link itself still
        // carries them to the desktop app).
        window.history.replaceState(null, '', '/desktop-handoff')

        const deepLink = `valori://auth-callback?${new URLSearchParams({
            access_token: accessToken,
            refresh_token: refreshToken,
        }).toString()}`
        window.location.href = deepLink
        setAttempted(true)
    }, [params])

    return (
        <div className="min-h-screen flex flex-col items-center justify-center gap-3 bg-background text-foreground p-4 text-center">
            <ShieldCheck className="text-primary" size={32} />
            <h1 className="text-lg font-semibold font-mono">SIGNED_IN</h1>
            <p className="text-sm text-muted-foreground max-w-sm">
                {attempted
                    ? 'You can close this window and return to the Valori desktop app.'
                    : 'Finishing sign-in…'}
            </p>
        </div>
    )
}

export default function DesktopHandoffPage() {
    return (
        <Suspense fallback={null}>
            <DesktopHandoffInner />
        </Suspense>
    )
}
