'use client'

import { login, signup } from './actions'
import { createClient } from '@/utils/supabase/client'
import { Code2, Globe, Activity, Shield, Lock, Server, Clock } from 'lucide-react'
import { useSearchParams } from 'next/navigation'
import { Suspense, useEffect, useState } from 'react'
import Link from 'next/link'
import { nativeAvailable, openCloudLogin } from '@/lib/native'
async function signInWithOAuth(provider: 'google' | 'github', next?: string | null) {
    if (nativeAvailable()) {
        await openCloudLogin(provider)
        return
    }

    const supabase = createClient()
    const canonicalOrigin = window.location.origin.replace('://www.', '://')
    const redirectUrl = `${canonicalOrigin}/auth/callback${next ? `?next=${encodeURIComponent(next)}` : ''}`

    const { data, error } = await supabase.auth.signInWithOAuth({
        provider,
        options: {
            redirectTo: redirectUrl,
            queryParams: {
                access_type: 'offline',
                prompt: 'consent',
            },
        },
    })

    if (error) {
        window.location.href = `/error?message=${encodeURIComponent(error.message)}`
        return
    }

    if (data.url) {
        window.location.href = data.url
    }
}

function LoginForm() {
    const searchParams = useSearchParams()
    const next = searchParams.get('next')
    const provider = searchParams.get('provider') as 'google' | 'github' | null

    // Auto-trigger OAuth when arriving from SignInGate with ?provider=google/github
    useEffect(() => {
        if (provider === 'google' || provider === 'github') {
            signInWithOAuth(provider, next)
        }
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])

    // Fake live clock for "Last Snapshot"
    const [time, setTime] = useState<string>("")

    useEffect(() => {
        const updateTime = () => {
            const now = new Date()
            setTime(now.toISOString().split('T')[1].split('.')[0] + " UTC")
        }
        updateTime()
        const interval = setInterval(updateTime, 1000)
        return () => clearInterval(interval)
    }, [])

    return (
        <div className="min-h-screen flex flex-col bg-background text-foreground font-sans selection:bg-primary/20">
            {/* Main Content */}
            <div className="flex-1 flex flex-col items-center justify-center p-4 mb-20">
                <div className="w-full max-w-md space-y-8">
                    <div className="text-center">
                        <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-primary/10 text-primary text-xs font-mono font-bold tracking-wider mb-6 border border-primary/20">
                            <Lock size={12} />
                            SECURE_ENVIRONMENT
                        </div>
                        <h2 className="text-3xl font-bold tracking-tight text-foreground font-mono">
                            TERMINAL_LOGIN
                        </h2>
                        <p className="mt-2 text-sm text-muted-foreground">
                            Authenticate significantly to access the deterministic kernel.
                        </p>
                    </div>

                    <form className="mt-8 space-y-6">
                        <input type="hidden" name="next" value={next ?? ''} />
                        <div className="space-y-4 rounded-md shadow-sm">
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
                            <div>
                                <label htmlFor="password" className="sr-only">Password</label>
                                <input
                                    id="password"
                                    name="password"
                                    type="password"
                                    autoComplete="current-password"
                                    required
                                    className="relative block w-full rounded-md border border-input bg-background px-3 py-3 text-foreground placeholder:text-muted-foreground focus:z-10 focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary sm:text-sm font-mono"
                                    placeholder="••••••••••••"
                                />
                            </div>
                            <div className="flex justify-end">
                                <Link
                                    href="/forgot-password"
                                    className="text-xs text-muted-foreground hover:text-primary transition-colors font-mono"
                                >
                                    Forgot password?
                                </Link>
                            </div>
                        </div>

                        <div className="flex flex-col gap-4">
                            <button formAction={login} className="group relative flex w-full justify-center items-center gap-2 rounded-none bg-primary text-black py-3 px-4 text-sm font-bold tracking-wider hover:bg-primary/90 focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2 transition-all font-mono uppercase">
                                <Lock size={16} />
                                Authenticate
                            </button>

                            <div className="relative flex py-2 items-center">
                                <div className="flex-grow border-t border-border"></div>
                                <span className="flex-shrink-0 mx-4 text-xs text-muted-foreground font-mono uppercase tracking-widest">Provisioned Access Only</span>
                                <div className="flex-grow border-t border-border"></div>
                            </div>

                            <button formAction={signup} className="group relative flex w-full justify-center rounded-none border border-input bg-transparent py-3 px-4 text-xs font-medium text-muted-foreground hover:text-foreground hover:border-foreground transition-colors font-mono uppercase tracking-wide">
                                Request Institutional Access
                            </button>
                        </div>
                    </form>

                    <div className="grid grid-cols-2 gap-4 mt-6">
                        <button
                            onClick={() => signInWithOAuth('google', next)}
                            className="flex items-center justify-center gap-2 rounded-md border border-input bg-card px-4 py-2 text-sm font-medium text-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
                        >
                            <Globe size={18} />
                            <span className="font-mono text-xs">SSO_GOOGLE</span>
                        </button>
                        <button
                            onClick={() => signInWithOAuth('github', next)}
                            className="flex items-center justify-center gap-2 rounded-md border border-input bg-card px-4 py-2 text-sm font-medium text-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
                        >
                            <Code2 size={18} />
                            <span className="font-mono text-xs">SSO_GITHUB</span>
                        </button>
                    </div>

                    <div className="text-center mt-4">
                        <Link
                            href="/"
                            className="text-xs text-muted-foreground hover:text-foreground transition-colors font-mono underline underline-offset-4"
                        >
                            &larr; BACK_TO_SIGN_IN_OPTIONS
                        </Link>
                    </div>

                    {/* Specialized Compliance Signaling */}
                    <div className="pt-8 border-t border-dashed border-border">
                        <div className="grid grid-cols-2 gap-6">
                            <div className="flex gap-3 items-start">
                                <Shield className="w-5 h-5 text-emerald-500 mt-0.5" />
                                <div>
                                    <h4 className="text-xs font-bold text-foreground font-mono">SEC 17a-4</h4>
                                    <p className="text-[10px] text-muted-foreground leading-tight mt-1">
                                        WORM (Write Once Read Many) Compliant Storage.
                                    </p>
                                </div>
                            </div>
                            <div className="flex gap-3 items-start">
                                <Activity className="w-5 h-5 text-emerald-500 mt-0.5" />
                                <div>
                                    <h4 className="text-xs font-bold text-foreground font-mono">SOC 2 Type II</h4>
                                    <p className="text-[10px] text-muted-foreground leading-tight mt-1">
                                        Audited Infrastructure & Security Controls.
                                    </p>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            {/* Live Metrics Footer */}
            <div className="fixed bottom-0 w-full border-t border-border bg-card/50 backdrop-blur-md p-2">
                <div className="max-w-7xl mx-auto flex flex-wrap justify-between items-center text-[10px] md:text-xs font-mono text-muted-foreground px-4 gap-4">
                    <div className="flex items-center gap-6">
                        <div className="flex items-center gap-2">
                            <Activity size={14} className="text-primary" />
                            <span className="uppercase tracking-wider">Kernel_Latency:</span>
                            <span className="text-primary font-bold">4µs</span>
                        </div>
                        <div className="flex items-center gap-2">
                            <Server size={14} className="text-emerald-500" />
                            <span className="uppercase tracking-wider">Node_Health:</span>
                            <span className="text-emerald-500 font-bold">100%</span>
                        </div>
                    </div>
                    <div className="flex items-center gap-2">
                        <Clock size={14} />
                        <span className="uppercase tracking-wider">Last_Snapshot:</span>
                        <span className="text-foreground">{time}</span>
                    </div>
                </div>
            </div>
        </div>
    )
}

export default function LoginPage() {
    return (
        <Suspense fallback={<div className="min-h-screen flex items-center justify-center font-mono text-sm">INITIALIZING_KERNEL...</div>}>
            <LoginForm />
        </Suspense>
    )
}
