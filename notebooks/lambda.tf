terraform {
  backend "local" {
    # put the state file in a volume that is persisted accross executions
    path = "/mnt/state_vol/terraform.tfstate"
  }
  required_version = ">=0.12"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 3.0"
    }
  }
}

variable "profile" {
  description = "The AWS profile (from credentials file) to use to deploy/modify/destroy this infra"
}

variable "region_name" {
  description = "The AWS region name (eu-west-1, us-east-2...) in which the stack will be deployed"
}

variable "binary_name" {
  description = "The name of the compiled binary to deploy"
}

provider "aws" {
  profile                 = var.profile
  region                  = var.region_name
  shared_credentials_file = "/creds"
}

module "lambda_function" {
  source = "terraform-aws-modules/lambda/aws"

  function_name = "cloud-reader-benchmark"
  description   = "Lambda to run benchmarks for the Rust Cloud Reader library"
  handler       = "N/A"
  runtime       = "provided"
  memory_size   = 3008
  timeout       = 60

  create_package         = false
  local_existing_package = "${var.binary_name}.zip"

  tags = {
    Name = "cloud-reader-benchmark"
  }
}

output "lambda_arn" {
  value       = module.lambda_function.lambda_function_arn
  description = "ARN of the benchmarking lambda function"
}
