output "api_url" {
  description = "Adresse du serveur, à mettre dans web/config.js et dans l'outil MJ (onglet Partage)."
  value       = aws_lambda_function_url.bazar.function_url
}

output "table" {
  value = aws_dynamodb_table.bazar.name
}
