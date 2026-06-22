# Deploying Valori on AWS (EKS)

Phase 3.9 — BYOC (Bring Your Own Cloud) deployment into a customer AWS VPC.

## Prerequisites

- AWS CLI configured (`aws configure`)
- Terraform >= 1.6
- kubectl
- Helm 3

## Quick start

```bash
cd terraform/aws

# Initialize providers and modules
terraform init

# Preview the plan
terraform plan \
  -var="cluster_name=valori-prod" \
  -var="aws_region=us-east-1"

# Apply (creates EKS cluster, VPC, S3 bucket, IAM roles, CloudWatch alarms)
terraform apply \
  -var="cluster_name=valori-prod" \
  -var="aws_region=us-east-1"

# Configure kubectl
$(terraform output -raw kubeconfig_command)

# Verify cluster
kubectl get nodes
```

## Variables

| Variable | Default | Description |
|---|---|---|
| `cluster_name` | `valori-prod` | Name prefix for all AWS resources |
| `aws_region` | `us-east-1` | AWS region |
| `environment` | `prod` | Environment tag |
| `eks_version` | `1.29` | Kubernetes version |
| `node_instance_type` | `r6i.xlarge` | EC2 instance type (memory-optimized recommended) |
| `node_min_count` | `3` | Minimum worker nodes |
| `node_max_count` | `9` | Maximum worker nodes |
| `node_disk_gb` | `100` | EBS root volume size per node (GB) |
| `high_availability` | `true` | One NAT gateway per AZ (set false to reduce cost in dev) |
| `s3_object_lock_days` | `90` | S3 Object Lock compliance retention (0 to disable) |
| `alert_sns_arns` | `[]` | SNS topics to notify for CloudWatch alarms |

## What gets created

| Resource | Purpose |
|---|---|
| EKS cluster | Kubernetes control plane |
| VPC + subnets | Isolated network (3 AZs) |
| NAT gateways | Private subnet outbound access |
| Worker node group | `r6i.xlarge` x 3 (autoscaling to 9) |
| S3 bucket | Snapshot offload + WAL archival (`VALORI_OBJECT_STORE_URL`) |
| S3 Object Lock | Compliance-mode retention (immutable audit trail) |
| S3 server-side encryption | KMS-encrypted at rest |
| IAM role (IRSA) | Pod-level S3 access — no long-lived credentials on nodes |
| ALB IAM role | AWS Load Balancer Controller |
| CloudWatch alarm: state hash | Fires when cluster nodes diverge for > 3 minutes |
| CloudWatch alarm: replication lag | Fires when follower lag exceeds threshold |

## Deploy Valori with Helm

After `terraform apply`:

```bash
# Get the S3 bucket and IRSA role from Terraform outputs
S3_BUCKET=$(terraform output -raw valori_object_store_url)
IRSA_ARN=$(terraform output -raw valori_irsa_role_arn)

# Install Valori (adapt your values.yaml)
helm upgrade --install valori ./helm/valori \
  --namespace valori --create-namespace \
  --set replicaCount=3 \
  --set env.VALORI_OBJECT_STORE_URL="$S3_BUCKET" \
  --set serviceAccount.annotations."eks\.amazonaws\.com/role-arn"="$IRSA_ARN"
```

## Monitoring

The Terraform module creates two CloudWatch alarms:

| Alarm | Metric | Threshold |
|---|---|---|
| `valori-state-hash-divergence` | `valori_state_hash_match` | < 1 for 3 minutes |
| `valori-replication-lag` | `valori_replication_lag_events` | > 1000 events |

Push Prometheus metrics to CloudWatch using the CloudWatch agent or
[amazon-cloudwatch-agent](https://github.com/aws/amazon-cloudwatch-agent) with
`emf_config` to emit custom metrics from Valori's `/metrics` endpoint.

## Backup and restore

Valori's object store integration (`VALORI_OBJECT_STORE_URL`) automatically
offloads snapshots to S3. To restore:

```bash
# List available snapshots
curl http://valori-lb/v1/storage/snapshots

# Restore from a specific snapshot key
curl -X POST http://valori-lb/v1/storage/snapshots/restore \
  -d '{"key": "snapshots/00000001750000000_abc12345.snap"}'
```

See [docs/DR.md](DR.md) for the full disaster-recovery runbook.

## Cost estimate (us-east-1, 3-node production cluster)

| Resource | Monthly (approx.) |
|---|---|
| 3x r6i.xlarge EKS nodes | ~$450 |
| NAT gateways (3x HA) | ~$100 |
| S3 storage (100 GB) | ~$3 |
| Data transfer | ~$20 |
| **Total** | **~$575/month** |

Reduce cost in non-production: `high_availability=false`, smaller instance type,
fewer nodes.

## Destruction

```bash
terraform destroy -var="cluster_name=valori-prod"
```

Note: The S3 bucket has `force_destroy = false` to prevent accidental data loss.
Empty the bucket manually before destroying if needed.
