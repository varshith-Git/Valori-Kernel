import { type NextRequest } from "next/server";
import { getMode } from "@/lib/server/api-client";
import { updateSession } from "@/utils/supabase/middleware";

// `middleware.ts` is deprecated as of Next.js 16 in favor of `proxy.ts` —
// see node_modules/next/dist/docs/.../file-conventions/proxy.md. Proxy
// defaults to the Node.js runtime (required here anyway: api-client.ts's
// getMode() reads a local file, which the old Edge default couldn't do).

export const config = {
    matcher: [
        "/((?!_next/static|_next/image|favicon.ico|.*\\.(?:svg|png|jpg|jpeg|gif|webp|mp4|csv|json)$).*)",
    ],
};

export async function proxy(request: NextRequest) {
    // Local mode has no Supabase session to refresh, and may not even have
    // NEXT_PUBLIC_SUPABASE_URL set — constructing a Supabase client would
    // throw. Only the cloud path (the website, or a desktop app that's
    // signed in to sync) touches Supabase at all.
    if (getMode() !== "cloud") return;

    return await updateSession(request);
}
