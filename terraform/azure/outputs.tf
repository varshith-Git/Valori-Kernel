output "cluster_name" {
  description = "AKS cluster name"
  value       = azurerm_kubernetes_cluster.valori.name
}

output "kube_config_raw" {
  description = "Raw kubeconfig for the AKS cluster"
  value       = azurerm_kubernetes_cluster.valori.kube_config_raw
  sensitive   = true
}

output "valori_storage_account" {
  description = "Azure Storage Account name for Valori object store"
  value       = azurerm_storage_account.valori.name
}

output "valori_storage_container" {
  description = "Azure Blob container name"
  value       = azurerm_storage_container.valori.name
}

output "key_vault_uri" {
  description = "Azure Key Vault URI (for CMK in Phase 5)"
  value       = azurerm_key_vault.valori.vault_uri
}

output "log_analytics_workspace_id" {
  description = "Log Analytics workspace ID (SOC 2 evidence store)"
  value       = azurerm_log_analytics_workspace.valori.id
}

output "valori_object_store_url" {
  description = "VALORI_OBJECT_STORE_URL env var value for this deployment (use azure:// scheme with opendal)"
  value       = "azblob://${azurerm_storage_container.valori.name}"
}

output "kubeconfig_command" {
  description = "Command to update kubeconfig for this cluster"
  value       = "az aks get-credentials --resource-group ${azurerm_resource_group.valori.name} --name ${azurerm_kubernetes_cluster.valori.name}"
}
