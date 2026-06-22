# Phase 3.9 — Terraform modules (AWS + Azure)

## Goal

BYOC (Bring Your Own Cloud) deployment modules so customers can run Valori in their own AWS or Azure VPCs with production-grade infrastructure, audit logging, and alerting — without Valori having access to their data.

## Delivered

### AWS (`terraform/aws/`)

| File | Contains |
|---|---|
| `main.tf` | EKS cluster, VPC (3 AZs), S3 bucket (Object Lock + KMS), IAM IRSA role, ALB controller role, CloudWatch alarms |
| `variables.tf` | All tuneable parameters with sane defaults |
| `outputs.tf` | `kubeconfig_command`, `valori_object_store_url`, `valori_irsa_role_arn`, `valori_s3_bucket` |

**Key design decisions:**
- S3 Object Lock (COMPLIANCE mode, 90-day default) — audit trail cannot be truncated even by bucket owner
- IRSA (IAM Roles for Service Accounts) — pod-level S3 access without long-lived credentials on nodes
- KMS server-side encryption on S3 by default
- Two CloudWatch alarms: `state_hash_match < 1` (divergence) and `replication_lag > 1000 events`

### Azure (`terraform/azure/`)

| File | Contains |
|---|---|
| `main.tf` | AKS cluster, Blob Storage (ZRS, versioning, lifecycle), Key Vault (purge-protected, Premium SKU), Log Analytics, AKS diagnostic settings, Monitor alerts |
| `variables.tf` | All tuneable parameters |
| `outputs.tf` | `kubeconfig_command`, `valori_object_store_url`, `key_vault_uri`, `log_analytics_workspace_id` |

**Key design decisions:**
- ZRS Blob Storage — zone-redundant by default; GRS available for cross-region DR
- Key Vault provisioned from day 1 for the Phase 5 CMK (Customer-Managed Keys) upgrade path — purge protection enabled
- Log Analytics workspace with 90-day retention for SOC 2 CC7.1/CC6.2 evidence
- AKS diagnostic settings: `kube-audit`, `kube-apiserver`, `cluster-autoscaler`

### Documentation

| File | Content |
|---|---|
| `docs/DEPLOY_AWS.md` | Quick-start, variables table, resource inventory, Helm deploy example, cost estimate (~$575/mo), destroy instructions |
| `docs/DEPLOY_AZURE.md` | Quick-start, variables table, resource inventory, SOC 2 KQL query examples, Key Vault CMK upgrade path, cost estimate (~$636/mo) |

## Findings

- Azure Blob Storage uses `azblob://container` scheme (opendal azure service). The `VALORI_OBJECT_STORE_URL` format for AWS is `s3://bucket/prefix` and for Azure is `azblob://container`.
- Azure Key Vault requires `purge_protection_enabled = true` for compliance workloads — this means the vault cannot be permanently deleted for 90 days after soft-delete. This is the correct behavior for audit key material.
- EKS node groups need the `kubernetes.io/role/internal-elb` tag on private subnets or the AWS Load Balancer Controller cannot provision NLBs for internal services.

## Validation

Terraform syntax and HCL formatting:
```bash
cd terraform/aws  && terraform fmt -check && terraform validate
cd terraform/azure && terraform fmt -check && terraform validate
```
(Requires `terraform init` first to download providers.)

## Follow-ups

- Phase 4.1 — Kubernetes operator: the Terraform modules provide the infrastructure; the operator manages the Valori StatefulSet lifecycle on top of it.
- GCP module: straightforward adaptation from the AWS module (GKE + GCS + Workload Identity).
