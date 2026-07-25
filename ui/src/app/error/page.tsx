import Link from 'next/link'
import { Suspense } from 'react'
import { AlertTriangle } from 'lucide-react'
import { ErrorMessage } from './ErrorMessage'

export default function ErrorPage() {
    return (
        <div className="min-h-screen flex flex-col items-center justify-center bg-background text-foreground p-4">
            <div className="w-full max-w-md text-center space-y-6">
                <div className="inline-flex items-center justify-center w-12 h-12 rounded-full bg-destructive/10 text-destructive mx-auto">
                    <AlertTriangle size={22} />
                </div>
                <div>
                    <h1 className="text-xl font-bold tracking-tight font-mono">AUTHENTICATION_ERROR</h1>
                    <Suspense fallback={null}>
                        <ErrorMessage />
                    </Suspense>
                </div>
                <Link
                    href="/login"
                    className="inline-block rounded-md border border-input bg-card px-4 py-2 text-sm font-medium text-foreground hover:bg-accent transition-colors"
                >
                    Back to login
                </Link>
            </div>
        </div>
    )
}
