variable "aws_region" {
  description = "AWS region to deploy into"
  type        = string
  default     = "us-east-1"
}

variable "cluster_name" {
  description = "Name prefix for all resources"
  type        = string
  default     = "valori-prod"
}

variable "environment" {
  description = "Environment tag (prod / staging / dev)"
  type        = string
  default     = "prod"
}

variable "eks_version" {
  description = "Kubernetes version for EKS"
  type        = string
  default     = "1.29"
}

variable "node_instance_type" {
  description = "EC2 instance type for EKS worker nodes"
  type        = string
  default     = "r6i.xlarge"
}

variable "node_min_count" {
  description = "Minimum worker nodes"
  type        = number
  default     = 3
}

variable "node_max_count" {
  description = "Maximum worker nodes"
  type        = number
  default     = 9
}

variable "node_desired_count" {
  description = "Desired worker nodes"
  type        = number
  default     = 3
}

variable "node_disk_gb" {
  description = "Root EBS volume size in GB per worker node"
  type        = number
  default     = 100
}

variable "vpc_cidr" {
  description = "VPC CIDR block"
  type        = string
  default     = "10.0.0.0/16"
}

variable "private_subnets" {
  description = "Private subnet CIDRs"
  type        = list(string)
  default     = ["10.0.1.0/24", "10.0.2.0/24", "10.0.3.0/24"]
}

variable "public_subnets" {
  description = "Public subnet CIDRs (for ALB)"
  type        = list(string)
  default     = ["10.0.101.0/24", "10.0.102.0/24", "10.0.103.0/24"]
}

variable "high_availability" {
  description = "Use one NAT gateway per AZ (true) or a single shared NAT (false)"
  type        = bool
  default     = true
}

variable "namespace" {
  description = "Kubernetes namespace for Valori workloads"
  type        = string
  default     = "valori"
}

variable "s3_object_lock_days" {
  description = "S3 Object Lock compliance retention in days (0 to disable)"
  type        = number
  default     = 90
}

variable "alert_sns_arns" {
  description = "SNS topic ARNs to notify on CloudWatch alarms"
  type        = list(string)
  default     = []
}

variable "replication_lag_alarm_events" {
  description = "Number of lagging events before the replication lag alarm fires"
  type        = number
  default     = 1000
}
