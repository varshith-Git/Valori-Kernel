variable "location" {
  description = "Azure region (e.g. eastus, westeurope)"
  type        = string
  default     = "eastus"
}

variable "resource_group_name" {
  description = "Azure resource group name"
  type        = string
  default     = "valori-prod-rg"
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

variable "aks_version" {
  description = "Kubernetes version for AKS"
  type        = string
  default     = "1.29"
}

variable "node_vm_size" {
  description = "Azure VM size for AKS worker nodes"
  type        = string
  default     = "Standard_E4s_v5"
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
  description = "Desired worker nodes at cluster creation"
  type        = number
  default     = 3
}

variable "node_disk_gb" {
  description = "Managed disk size in GB per worker node"
  type        = number
  default     = 128
}

variable "storage_replication_type" {
  description = "Azure Storage Account replication (LRS, ZRS, GRS, RAGRS)"
  type        = string
  default     = "ZRS"
}

variable "snapshot_retention_days" {
  description = "Days to retain snapshot blobs before deletion (lifecycle policy)"
  type        = number
  default     = 365
}

variable "log_retention_days" {
  description = "Log Analytics workspace retention in days (SOC 2 evidence)"
  type        = number
  default     = 90
}

variable "alert_email_addresses" {
  description = "Email addresses to notify on metric alerts"
  type        = list(string)
  default     = []
}
