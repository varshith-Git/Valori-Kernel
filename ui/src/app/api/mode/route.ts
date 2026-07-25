import { NextResponse } from "next/server";
import { getMode, setMode } from "@/lib/server/api-client";

// Not a Server Action because it's called from a plain client-side fetch in
// the "sign in to sync" handoff page, before that page necessarily has a
// live Supabase session established yet.
export async function GET() {
    return NextResponse.json({ mode: getMode() });
}

export async function POST(req: Request) {
    const { mode } = await req.json();
    if (mode !== "local" && mode !== "cloud") {
        return NextResponse.json({ error: "mode must be 'local' or 'cloud'" }, { status: 400 });
    }
    setMode(mode);
    return NextResponse.json({ mode });
}
