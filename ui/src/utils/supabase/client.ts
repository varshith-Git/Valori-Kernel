import { createBrowserClient } from '@supabase/ssr'
import { supabaseCookieOptions } from './cookieOptions'

export function createClient() {
    return createBrowserClient(
        process.env.NEXT_PUBLIC_SUPABASE_URL!,
        process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY!,
        {
            cookieOptions: supabaseCookieOptions(typeof window !== 'undefined' ? window.location.hostname : undefined),
            auth: {
                persistSession: true,
                storage: typeof window !== 'undefined' ? {
                    getItem: (key: string) => {
                        const local = window.localStorage.getItem(key);
                        if (local) return local;
                        const match = document.cookie.match(new RegExp('(^| )' + key + '=([^;]+)'));
                        if (match) return decodeURIComponent(match[2]);
                        return null;
                    },
                    setItem: (key: string, value: string) => {
                        window.localStorage.setItem(key, value);
                        const hostname = window.location.hostname;
                        const isProd = hostname.endsWith('valori.systems');
                        const domainAttr = isProd ? `; domain=.valori.systems` : '';
                        const secureAttr = isProd ? '; secure' : '';
                        document.cookie = `${key}=${encodeURIComponent(value)}; path=/${domainAttr}${secureAttr}; max-age=31536000; SameSite=Lax`;
                    },
                    removeItem: (key: string) => {
                        window.localStorage.removeItem(key);
                        const hostname = window.location.hostname;
                        const isProd = hostname.endsWith('valori.systems');
                        const domainAttr = isProd ? `; domain=.valori.systems` : '';
                        document.cookie = `${key}=; path=/${domainAttr}; expires=Thu, 01 Jan 1970 00:00:00 GMT`;
                    }
                } : undefined
            }
        }
    )
}
