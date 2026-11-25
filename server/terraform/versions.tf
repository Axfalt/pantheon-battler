terraform {
  required_version = ">= 1.6"
  required_providers {
    # invoked_via_function_url (aws_lambda_permission) needs a recent 6.x provider.
    aws     = { source = "hashicorp/aws", version = "~> 6.0" }
    archive = { source = "hashicorp/archive", version = "~> 2.4" }
  }
}

provider "aws" {
  region = var.region
  default_tags {
    tags = { Project = "pantheon-battler", Part = "bazar" }
  }
}
