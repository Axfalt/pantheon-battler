# Pantheon Battler · the Bazar server, built to stay inside AWS's permanent free allowances (not
# the 12-month ones): a Lambda function reached through its own address (Function URL, no API
# Gateway) and a DynamoDB table with a small fixed capacity (provisioned, not billed per
# request). Every item expires var.ttl_days after its last write.

# ---------------------------------------------------------------- Data
resource "aws_dynamodb_table" "bazar" {
  name           = var.name
  billing_mode   = "PROVISIONED"
  read_capacity  = var.read_capacity
  write_capacity = var.write_capacity
  hash_key       = "pk"
  range_key      = "sk"

  attribute {
    name = "pk"
    type = "S"
  }
  attribute {
    name = "sk"
    type = "S"
  }

  ttl {
    attribute_name = "ttl"
    enabled        = true
  }

  # Continuous backup: about 0.20 $ per GB and per month, under a cent for a few MB.
  point_in_time_recovery {
    enabled = true
  }
}

# ---------------------------------------------------------------- Code
# The AWS SDK comes with the Node.js runtime: only our files go in the zip.
data "archive_file" "code" {
  type        = "zip"
  output_path = "${path.module}/build/bazar.zip"
  dynamic "source" {
    for_each = ["lambda.mjs", "handler.mjs", "store-dynamo.mjs", "myhordes.mjs"]
    content {
      filename = source.value
      content  = file("${path.module}/../${source.value}")
    }
  }
}

resource "aws_iam_role" "bazar" {
  name = var.name
  assume_role_policy = jsonencode({
    Version   = "2012-10-17"
    Statement = [{ Effect = "Allow", Principal = { Service = "lambda.amazonaws.com" }, Action = "sts:AssumeRole" }]
  })
}

resource "aws_iam_role_policy" "bazar" {
  name = "${var.name}-access"
  role = aws_iam_role.bazar.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect   = "Allow"
        Action   = ["dynamodb:GetItem", "dynamodb:BatchGetItem", "dynamodb:Query", "dynamodb:PutItem", "dynamodb:UpdateItem", "dynamodb:DeleteItem", "dynamodb:ConditionCheckItem"]
        Resource = aws_dynamodb_table.bazar.arn
      },
      {
        Effect   = "Allow"
        Action   = ["logs:CreateLogStream", "logs:PutLogEvents"]
        Resource = "${aws_cloudwatch_log_group.bazar.arn}:*"
      },
    ]
  })
}

resource "aws_cloudwatch_log_group" "bazar" {
  name              = "/aws/lambda/${var.name}"
  retention_in_days = 14
}

resource "aws_lambda_function" "bazar" {
  function_name                  = var.name
  role                           = aws_iam_role.bazar.arn
  runtime                        = "nodejs22.x"
  architectures                  = ["arm64"]
  handler                        = "lambda.handler"
  filename                       = data.archive_file.code.output_path
  source_code_hash               = data.archive_file.code.output_base64sha256
  memory_size                    = 256
  timeout                        = 10
  reserved_concurrent_executions = var.max_concurrency > 0 ? var.max_concurrency : -1

  environment {
    variables = {
      TABLE_NAME = aws_dynamodb_table.bazar.name
      GM_KEY     = var.gm_key
      MH_APPKEY  = var.myhordes_app_key
      TTL_DAYS   = tostring(var.ttl_days)
    }
  }

  depends_on = [aws_cloudwatch_log_group.bazar, aws_iam_role_policy.bazar]
}

# ---------------------------------------------------------------- Public address
resource "aws_lambda_function_url" "bazar" {
  function_name      = aws_lambda_function.bazar.function_name
  authorization_type = "NONE"

  # The browser's CORS checks are answered here, without running the function.
  cors {
    allow_origins = [var.allowed_origin]
    allow_methods = ["GET", "POST", "PUT", "DELETE"]
    allow_headers = ["content-type", "authorization"]
    max_age       = 3600
  }
}

# A public address (authorization NONE) needs both permissions; the second only through the
# URL, so nobody can call the function another way.
resource "aws_lambda_permission" "url" {
  statement_id           = "FunctionUrlPublic"
  action                 = "lambda:InvokeFunctionUrl"
  function_name          = aws_lambda_function.bazar.function_name
  principal              = "*"
  function_url_auth_type = "NONE"
}

resource "aws_lambda_permission" "invoke" {
  statement_id             = "FunctionUrlInvoke"
  action                   = "lambda:InvokeFunction"
  function_name            = aws_lambda_function.bazar.function_name
  principal                = "*"
  invoked_via_function_url = true
}

# ---------------------------------------------------------------- Spending alert
resource "aws_budgets_budget" "alert" {
  count        = var.alert_email == "" ? 0 : 1
  name         = "${var.name}-1-dollar"
  budget_type  = "COST"
  limit_amount = "1"
  limit_unit   = "USD"
  time_unit    = "MONTHLY"

  # 1 % of 1 $: the first cent.
  notification {
    comparison_operator        = "GREATER_THAN"
    threshold                  = 1
    threshold_type             = "PERCENTAGE"
    notification_type          = "ACTUAL"
    subscriber_email_addresses = [var.alert_email]
  }
}
