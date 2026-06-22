# Deploying Valori on Azure (AKS)

Phase 3.9 — BYOC (Bring Your Own Cloud) deployment into a customer Azure subscription.

## Prerequisites

- Azure CLI (`az login`)
- Terraform >= 1.6
- kubectl
- Helm 3

## Quick start

```bash
cd terraform/azure

# Initialize providers and modules
terraform init

# Preview the plan
terraform plan \
  -var="cluster_name=valori-prod" \
  -var="location=eastus" \
  -var="resource_group_name=valori-prod-rg"

# Apply (creates AKS cluster, Blob Storage, Key Vault, Log Analytics, alerts)
terraform apply \
  -var="cluster_name=valori-prod" \
  -var="location=eastus" \
  -var="resource_group_name=valori-prod-rg"

# Configure kubectl
$(terraform output -raw kubeconfig_command)

# Verify cluster
kubectl get nodes
```

## Variables

| Variable | Default | Description |
|---|---|---|
| `location` | `eastus` | Azure region |
| `resource_group_name` | `valori-prod-rg` | Azure resource group |
| `cluster_name` | `valori-prod` | Name prefix for all resources |
| `environment` | `prod` | Environment tag |
| `aks_version` | `1.29` | Kubernetes version |
| `node_vm_size` | `Standard_E4s_v5` | Azure VM size (memory-optimized) |
| `node_min_count` | `3` | Minimum worker nodes |
| `node_max_count` | `9` | Maximum worker nodes |
| `node_disk_gb` | `128` | Managed disk size per node (GB) |
| `storage_replication_type` | `ZRS` | Storage redundancy (LRS / ZRS / GRS) |
| `snapshot_retention_days` | `365` | Blob lifecycle retention |
| `log_retention_days` | `90` | Log Analytics retention (SOC 2 evidence) |
| `alert_email_addresses` | `[]` | Email recipients for metric alerts |

## What gets created

| Resource | Purpose |
|---|---|
| AKS cluster | Kubernetes control plane |
| Default node pool | `Standard_E4s_v5` x 3 (autoscaling to 9) |
| Azure Blob Storage | Snapshot offload + WAL archival |
| ZRS replication | Zone-redundant storage (3-copy durability) |
| Blob versioning | Soft-delete and version history |
| Lifecycle policy | Auto-tier old snapshots to Cool/Archive; delete after 365 days |
| Azure Key Vault | CMK integration (Phase 5); purge-protection enabled |
| Log Analytics | AKS diagnostic logs + audit trail (SOC 2 CC7.2) |
| Diagnostic settings | `kube-audit`, `kube-apiserver`, `cluster-autoscaler` logs |
| Monitor alert | State hash divergence alert → email |

## Deploy Valori with Helm

```bash
# Get the storage account name from Terraform outputs
STORAGE_ACCOUNT=$(terraform output -raw valori_storage_account)
CONTAINER=$(terraform output -raw valori_storage_container)

# Set up Workload Identity (or use a storage connection string for simplicity)
# See: https://learn.microsoft.com/en-us/azure/aks/workload-identity-overview

helm upgrade --install valori ./helm/valori \
  --namespace valori --create-namespace \
  --set replicaCount=3 \
  --set env.VALORI_OBJECT_STORE_URL="azblob://${CONTAINER}" \
  --set env.AZURE_STORAGE_ACCOUNT_NAME="$STORAGE_ACCOUNT"
```

## SOC 2 evidence trail

The Log Analytics workspace collects:

| Log category | SOC 2 control |
|---|---|
| `kube-audit` | CC6.2 — Access control / authentication |
| `kube-apiserver` | CC7.1 — System monitoring |
| `cluster-autoscaler` | A1.2 — Capacity management |

Retention is set to `log_retention_days` (default 90) to ensure a full quarter
of evidence is always available.

Query recent control-plane events in Azure Monitor:

```kql
AzureDiagnostics
| where Category == "kube-audit"
| where TimeGenerated > ago(7d)
| project TimeGenerated, RequestURI, ResponseStatus
| order by TimeGenerated desc
```

## Key Vault (Phase 5 CMK upgrade)

The Key Vault is provisioned with:
- Purge protection enabled (prevents accidental permanent deletion)
- RBAC authorization (granular role assignments instead of vault-level policies)
- Premium SKU (supports HSM-backed keys for Phase 5)

When Phase 5 CMK is ready, assign the `Key Vault Crypto Officer` role to the
AKS managed identity and configure Storage Account encryption to use the vault key.

## Backup and restore

```bash
# List snapshots
curl http://valori-lb/v1/storage/snapshots

# Restore from a snapshot
curl -X POST http://valori-lb/v1/storage/snapshots/restore \
  -d '{"key": "snapshots/00000001750000000_abc12345.snap"}'
```

## Cost estimate (East US, 3-node production cluster)

| Resource | Monthly (approx.) |
|---|---|
| 3x Standard_E4s_v5 AKS nodes | ~$600 |
| ZRS Blob Storage (100 GB) | ~$5 |
| Log Analytics (10 GB/day) | ~$30 |
| Key Vault (1000 operations/day) | ~$1 |
| **Total** | **~$636/month** |

## Destruction

```bash
terraform destroy \
  -var="cluster_name=valori-prod" \
  -var="resource_group_name=valori-prod-rg"
```
