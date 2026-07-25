'use client'

import { useSearchParams } from 'next/navigation'

export function ErrorMessage() {
    const message = useSearchParams().get('message')
    return (
        <p className="mt-2 text-sm text-muted-foreground">
            {message || 'Something went wrong during authentication. Please try again.'}
        </p>
    )
}
