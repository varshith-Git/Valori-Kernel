import { redirect } from 'next/navigation'

export default function SettingsIndexPage() {
    redirect('/cloud/settings/api-keys')
}
