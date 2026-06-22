terraform {
  required_version = ">= 1.6"
  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = "~> 3.100"
    }
    azuread = {
      source  = "hashicorp/azuread"
      version = "~> 2.50"
    }
    kubernetes = {
      source  = "hashicorp/kubernetes"
      version = "~> 2.27"
    }
  }
}

provider "azurerm" {
  features {
    key_vault {
      purge_soft_delete_on_destroy = false
    }
  }
}

data "azurerm_client_config" "current" {}

# ── Resource Group ────────────────────────────────────────────────────────────

resource "azurerm_resource_group" "valori" {
  name     = var.resource_group_name
  location = var.location
  tags     = local.common_tags
}

# ── AKS Cluster ───────────────────────────────────────────────────────────────

resource "azurerm_kubernetes_cluster" "valori" {
  name                = var.cluster_name
  location            = azurerm_resource_group.valori.location
  resource_group_name = azurerm_resource_group.valori.name
  dns_prefix          = var.cluster_name
  kubernetes_version  = var.aks_version

  default_node_pool {
    name                = "valori"
    node_count          = var.node_desired_count
    min_count           = var.node_min_count
    max_count           = var.node_max_count
    enable_auto_scaling = true
    vm_size             = var.node_vm_size

    os_disk_type    = "Managed"
    os_disk_size_gb = var.node_disk_gb

    upgrade_settings {
      max_surge = "10%"
    }
  }

  identity {
    type = "SystemAssigned"
  }

  network_profile {
    network_plugin    = "azure"
    load_balancer_sku = "standard"
  }

  oms_agent {
    log_analytics_workspace_id = azurerm_log_analytics_workspace.valori.id
  }

  tags = local.common_tags
}

# ── Azure Blob Storage for snapshots + WAL archival ───────────────────────────

resource "azurerm_storage_account" "valori" {
  name                     = replace(lower("${var.cluster_name}store"), "-", "")
  resource_group_name      = azurerm_resource_group.valori.name
  location                 = azurerm_resource_group.valori.location
  account_tier             = "Standard"
  account_replication_type = var.storage_replication_type

  blob_properties {
    versioning_enabled = true
  }

  # Encryption at rest with CMK via Key Vault (Phase 5 upgrade path).
  # Default: Microsoft-managed keys.

  tags = local.common_tags
}

resource "azurerm_storage_container" "valori" {
  name                  = "valori"
  storage_account_name  = azurerm_storage_account.valori.name
  container_access_type = "private"
}

resource "azurerm_storage_management_policy" "valori" {
  storage_account_id = azurerm_storage_account.valori.id

  rule {
    name    = "archive-old-snapshots"
    enabled = true
    filters {
      prefix_match = ["valori/snapshots/"]
      blob_types   = ["blockBlob"]
    }
    actions {
      base_blob {
        tier_to_cool_after_days_since_modification_greater_than    = 30
        tier_to_archive_after_days_since_modification_greater_than = 90
        delete_after_days_since_modification_greater_than          = var.snapshot_retention_days
      }
    }
  }
}

# ── Key Vault (CMK for Phase 5, available from Phase 3.9) ─────────────────────

resource "azurerm_key_vault" "valori" {
  name                        = "${var.cluster_name}-kv"
  location                    = azurerm_resource_group.valori.location
  resource_group_name         = azurerm_resource_group.valori.name
  tenant_id                   = data.azurerm_client_config.current.tenant_id
  sku_name                    = "premium"
  soft_delete_retention_days  = 90
  purge_protection_enabled    = true
  enable_rbac_authorization   = true

  tags = local.common_tags
}

# ── Log Analytics (for AKS monitoring + SOC 2 evidence) ──────────────────────

resource "azurerm_log_analytics_workspace" "valori" {
  name                = "${var.cluster_name}-logs"
  location            = azurerm_resource_group.valori.location
  resource_group_name = azurerm_resource_group.valori.name
  sku                 = "PerGB2018"
  retention_in_days   = var.log_retention_days

  tags = local.common_tags
}

resource "azurerm_monitor_diagnostic_setting" "aks" {
  name               = "${var.cluster_name}-aks-diag"
  target_resource_id = azurerm_kubernetes_cluster.valori.id

  log_analytics_workspace_id = azurerm_log_analytics_workspace.valori.id

  enabled_log {
    category = "kube-audit"
  }
  enabled_log {
    category = "kube-apiserver"
  }
  enabled_log {
    category = "cluster-autoscaler"
  }
}

# ── Alerts ────────────────────────────────────────────────────────────────────

resource "azurerm_monitor_action_group" "valori" {
  count               = length(var.alert_email_addresses) > 0 ? 1 : 0
  name                = "${var.cluster_name}-alerts"
  resource_group_name = azurerm_resource_group.valori.name
  short_name          = "valori"

  dynamic "email_receiver" {
    for_each = var.alert_email_addresses
    content {
      name          = "email-${email_receiver.key}"
      email_address = email_receiver.value
    }
  }
}

resource "azurerm_monitor_metric_alert" "state_hash_divergence" {
  count               = length(var.alert_email_addresses) > 0 ? 1 : 0
  name                = "${var.cluster_name}-state-hash-divergence"
  resource_group_name = azurerm_resource_group.valori.name
  scopes              = [azurerm_kubernetes_cluster.valori.id]
  description         = "Valori cluster nodes have diverged state hashes"
  severity            = 2
  frequency           = "PT1M"
  window_size         = "PT5M"

  criteria {
    metric_namespace = "Microsoft.ContainerService/managedClusters"
    metric_name      = "valori_state_hash_match"
    aggregation      = "Average"
    operator         = "LessThan"
    threshold        = 1
  }

  action {
    action_group_id = azurerm_monitor_action_group.valori[0].id
  }
}

# ── Locals ────────────────────────────────────────────────────────────────────

locals {
  common_tags = {
    Project     = "valori"
    Environment = var.environment
    ManagedBy   = "terraform"
  }
}
