import { createBrowserClient } from '@supabase/ssr'
import { supabaseCookieOptions } from './cookieOptions'

export function createClient() {
    return createBrowserClient(
        process.env.NEXT_PUBLIC_SUPABASE_URL!,
        process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY!,
        { cookieOptions: supabaseCookieOptions(typeof window !== 'undefined' ? window.location.hostname : undefined) }
    )
}
