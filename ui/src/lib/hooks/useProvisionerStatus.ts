'use client'

import useSWR from 'swr'

// Distinct from useHealth: this asks the Rust backend's Provisioner what it
// thinks each instance's container status is (via Dokploy/mock), not the
// valori-node's own /health. A project can be "active" in Supabase with a
// dead container — this is what would actually catch that.
export interface InstanceStatusEntry {
    instance_id: string
    host_id: string
    node_index: number
    status: string
}

interface ProjectStatusResponse {
    project_id: string
    instances: InstanceStatusEntry[]
}

const fetcher = (url: string) => fetch(url).then((r) => r.json() as Promise<ProjectStatusResponse>)

export function useProvisionerStatus(projectId: string) {
    const { data, error, isLoading, mutate } = useSWR<ProjectStatusResponse>(
        `/api/cloud/projects/${projectId}/status`,
        fetcher,
        { refreshInterval: 10000 }
    )

    return {
        instances: data?.instances ?? [],
        error: error ?? null,
        isLoading,
        refresh: mutate,
    }
}
