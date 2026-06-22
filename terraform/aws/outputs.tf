output "cluster_name" {
  description = "EKS cluster name"
  value       = module.eks.cluster_name
}

output "cluster_endpoint" {
  description = "EKS cluster API endpoint"
  value       = module.eks.cluster_endpoint
}

output "cluster_certificate_authority_data" {
  description = "EKS cluster CA (base64)"
  value       = module.eks.cluster_certificate_authority_data
  sensitive   = true
}

output "valori_s3_bucket" {
  description = "S3 bucket name for Valori object store"
  value       = aws_s3_bucket.valori_store.id
}

output "valori_irsa_role_arn" {
  description = "IAM role ARN for Valori pods (IRSA)"
  value       = module.valori_irsa.iam_role_arn
}

output "kubeconfig_command" {
  description = "Command to update kubeconfig for this cluster"
  value       = "aws eks update-kubeconfig --region ${var.aws_region} --name ${module.eks.cluster_name}"
}

output "valori_object_store_url" {
  description = "VALORI_OBJECT_STORE_URL env var value for this deployment"
  value       = "s3://${aws_s3_bucket.valori_store.id}/valori"
}
