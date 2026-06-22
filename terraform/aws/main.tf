terraform {
  required_version = ">= 1.6"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
    kubernetes = {
      source  = "hashicorp/kubernetes"
      version = "~> 2.27"
    }
    helm = {
      source  = "hashicorp/helm"
      version = "~> 2.13"
    }
  }
}

provider "aws" {
  region = var.aws_region
}

# ── Data sources ──────────────────────────────────────────────────────────────

data "aws_availability_zones" "available" {
  state = "available"
}

data "aws_caller_identity" "current" {}

# ── EKS Cluster ──────────────────────────────────────────────────────────────

module "eks" {
  source  = "terraform-aws-modules/eks/aws"
  version = "~> 20.0"

  cluster_name    = var.cluster_name
  cluster_version = var.eks_version

  vpc_id     = module.vpc.vpc_id
  subnet_ids = module.vpc.private_subnets

  cluster_endpoint_public_access = true

  eks_managed_node_groups = {
    valori = {
      instance_types = [var.node_instance_type]
      min_size       = var.node_min_count
      max_size       = var.node_max_count
      desired_size   = var.node_desired_count

      block_device_mappings = {
        xvda = {
          device_name = "/dev/xvda"
          ebs = {
            volume_type = "gp3"
            volume_size = var.node_disk_gb
            encrypted   = true
          }
        }
      }

      labels = {
        workload = "valori"
      }

      taints = []
    }
  }

  tags = local.common_tags
}

# ── VPC ───────────────────────────────────────────────────────────────────────

module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "~> 5.0"

  name = "${var.cluster_name}-vpc"
  cidr = var.vpc_cidr

  azs             = slice(data.aws_availability_zones.available.names, 0, 3)
  private_subnets = var.private_subnets
  public_subnets  = var.public_subnets

  enable_nat_gateway   = true
  single_nat_gateway   = !var.high_availability
  enable_dns_hostnames = true

  public_subnet_tags = {
    "kubernetes.io/role/elb" = 1
  }

  private_subnet_tags = {
    "kubernetes.io/role/internal-elb" = 1
  }

  tags = local.common_tags
}

# ── S3 bucket for snapshots + WAL archival ────────────────────────────────────

resource "aws_s3_bucket" "valori_store" {
  bucket        = "${var.cluster_name}-valori-store-${data.aws_caller_identity.current.account_id}"
  force_destroy = false

  tags = merge(local.common_tags, { Name = "valori-object-store" })
}

resource "aws_s3_bucket_versioning" "valori_store" {
  bucket = aws_s3_bucket.valori_store.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_object_lock_configuration" "valori_store" {
  bucket = aws_s3_bucket.valori_store.id

  rule {
    default_retention {
      mode = "COMPLIANCE"
      days = var.s3_object_lock_days
    }
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "valori_store" {
  bucket = aws_s3_bucket.valori_store.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "aws:kms"
    }
    bucket_key_enabled = true
  }
}

resource "aws_s3_bucket_public_access_block" "valori_store" {
  bucket                  = aws_s3_bucket.valori_store.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# ── IAM role for Valori pods (IRSA) ──────────────────────────────────────────

data "aws_iam_policy_document" "valori_s3" {
  statement {
    actions = [
      "s3:PutObject",
      "s3:GetObject",
      "s3:ListBucket",
      "s3:DeleteObject",
    ]
    resources = [
      aws_s3_bucket.valori_store.arn,
      "${aws_s3_bucket.valori_store.arn}/*",
    ]
  }
}

resource "aws_iam_policy" "valori_s3" {
  name        = "${var.cluster_name}-valori-s3"
  description = "Allows Valori pods to read/write the object store bucket"
  policy      = data.aws_iam_policy_document.valori_s3.json
}

module "valori_irsa" {
  source  = "terraform-aws-modules/iam/aws//modules/iam-role-for-service-accounts-eks"
  version = "~> 5.0"

  role_name = "${var.cluster_name}-valori"

  attach_policies_as_iam_policy_arns = [aws_iam_policy.valori_s3.arn]

  oidc_providers = {
    main = {
      provider_arn               = module.eks.oidc_provider_arn
      namespace_service_accounts = ["${var.namespace}:valori"]
    }
  }
}

# ── ALB Ingress Controller ────────────────────────────────────────────────────

module "alb_irsa" {
  source  = "terraform-aws-modules/iam/aws//modules/iam-role-for-service-accounts-eks"
  version = "~> 5.0"

  role_name                              = "${var.cluster_name}-alb-controller"
  attach_load_balancer_controller_policy = true

  oidc_providers = {
    main = {
      provider_arn               = module.eks.oidc_provider_arn
      namespace_service_accounts = ["kube-system:aws-load-balancer-controller"]
    }
  }
}

# ── CloudWatch alarms ─────────────────────────────────────────────────────────

resource "aws_cloudwatch_metric_alarm" "state_hash_divergence" {
  alarm_name          = "${var.cluster_name}-valori-state-hash-divergence"
  comparison_operator = "LessThanThreshold"
  evaluation_periods  = 3
  metric_name         = "valori_state_hash_match"
  namespace           = "Valori"
  period              = 60
  statistic           = "Average"
  threshold           = 1

  alarm_description = "Valori cluster nodes have diverged state hashes for > 3 minutes"
  alarm_actions     = var.alert_sns_arns
  ok_actions        = var.alert_sns_arns

  treat_missing_data = "breaching"
  tags               = local.common_tags
}

resource "aws_cloudwatch_metric_alarm" "replication_lag" {
  alarm_name          = "${var.cluster_name}-valori-replication-lag"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 2
  metric_name         = "valori_replication_lag_events"
  namespace           = "Valori"
  period              = 60
  statistic           = "Maximum"
  threshold           = var.replication_lag_alarm_events

  alarm_description = "Valori follower is lagging by more than ${var.replication_lag_alarm_events} events"
  alarm_actions     = var.alert_sns_arns

  treat_missing_data = "notBreaching"
  tags               = local.common_tags
}

# ── Locals ────────────────────────────────────────────────────────────────────

locals {
  common_tags = {
    Project     = "valori"
    Environment = var.environment
    ManagedBy   = "terraform"
  }
}
