import { CopyBtn } from '@/components/ui/copy-btn'

const PIP_INSTALL = 'pip install valoricore'

const QUICKSTART = `from valoricore.remote import SyncRemoteClient

client = SyncRemoteClient(
    "https://your-project.valori.systems",
    api_key="vlk_...",
)

client.insert([0.1, 0.2, 0.3], text="hello world")
results = client.search([0.1, 0.2, 0.3], k=5)
`

const CURL_QUICKSTART = `curl -X POST https://your-project.valori.systems/search \\
  -H "Authorization: Bearer vlk_..." \\
  -H "Content-Type: application/json" \\
  -d '{"query": [0.1, 0.2, 0.3], "k": 5}'
`

export function SdkDownloads() {
    return (
        <div className="rounded-xl border border-border bg-card p-5 space-y-5">
            <div>
                <h2 className="text-sm font-semibold text-foreground">SDKs</h2>
                <p className="text-xs text-muted-foreground mt-0.5">
                    Python is the only published client SDK today — every project also speaks plain HTTP, so curl (or
                    any HTTP client) works without one. JS/TS and Go clients aren&apos;t built yet.
                </p>
            </div>

            <div className="space-y-2">
                <div className="flex items-center justify-between">
                    <p className="text-xs font-medium text-foreground">Python</p>
                    <CopyBtn text={PIP_INSTALL} label="copy" className="scale-90 origin-right" />
                </div>
                <pre className="rounded-lg border border-border bg-background/60 px-3 py-2 font-mono text-xs text-foreground overflow-x-auto">
                    {PIP_INSTALL}
                </pre>
            </div>

            <div className="space-y-2">
                <div className="flex items-center justify-between">
                    <p className="text-xs font-medium text-foreground">Quickstart (Python)</p>
                    <CopyBtn text={QUICKSTART} label="copy" className="scale-90 origin-right" />
                </div>
                <pre className="rounded-lg border border-border bg-background/60 px-3 py-3 font-mono text-xs text-foreground overflow-x-auto whitespace-pre">
                    {QUICKSTART}
                </pre>
            </div>

            <div className="space-y-2">
                <div className="flex items-center justify-between">
                    <p className="text-xs font-medium text-foreground">Quickstart (curl — no SDK required)</p>
                    <CopyBtn text={CURL_QUICKSTART} label="copy" className="scale-90 origin-right" />
                </div>
                <pre className="rounded-lg border border-border bg-background/60 px-3 py-3 font-mono text-xs text-foreground overflow-x-auto whitespace-pre">
                    {CURL_QUICKSTART}
                </pre>
            </div>
        </div>
    )
}
