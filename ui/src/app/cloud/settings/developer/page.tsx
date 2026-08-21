import { createClient } from '@/utils/supabase/server'
import { getAuthedUser } from '@/utils/supabase/dal'
import { redirect } from 'next/navigation'
import { SettingsNav } from '../SettingsNav'
import { DeveloperManager } from './DeveloperManager'
import { SdkDownloads } from './SdkDownloads'

export default async function DeveloperSettingsPage() {
    const supabase = await createClient()
    const user = await getAuthedUser()

    if (!user) {
        redirect('/login')
    }

    const { data: tokens } = await supabase
        .from('personal_access_tokens_public')
        .select('*')
        .order('created_at', { ascending: false })

    return (
        <div className="min-h-screen p-4 sm:p-8">
            <div className="w-full max-w-3xl mx-auto space-y-6">
                <div>
                    <h1 className="text-2xl font-bold tracking-tight text-foreground">Settings</h1>
                    <p className="text-sm text-muted-foreground mt-1">
                        Personal access tokens and SDKs for building against Valori directly.
                    </p>
                </div>

                <SettingsNav />

                <SdkDownloads />

                <DeveloperManager initialTokens={tokens ?? []} />
            </div>
        </div>
    )
}
