# FamilyLedger — AWS SAM Infrastructure Stacks

This directory contains Infrastructure-as-Code (IaC) templates for AWS SAM:

| Stack Name | Template File | Config File | Description | Idle Cost |
| :--- | :--- | :--- | :--- | :--- |
| `familyledger-core-dev` | `template.yaml` | `samconfig.toml` | Cognito User Pool (identity only; tenant context via `X-Family-Id`), Web Client, S3 SPA Bucket, CloudFront CDN, API Gateway HTTP API | **$0.00** |
| `familyledger-data-dev` | `data-stream-template.yaml` | `samconfig-data.toml` | Custom EventBridge Event Bus (`familyledger-events-dev`) and least-privilege IAM Execution Roles (`FamilyLedgerServiceRole`, `FamilyLedgerMigrationRunnerRole`) | **$0.00** |

> [!NOTE]
> **Event Publishing Architecture (ADR-0017)**: Amazon Kinesis Data Stream is disabled/removed due to fixed idle hourly fees ($10.80–$28.80/month). Rust service Lambdas publish domain events directly to the EventBridge bus (`events:PutEvents`) at **$0.00 idle cost**.

---

## 1. Core Stack (`familyledger-core-dev`) — Recreating Cleanly

Amazon Cognito does not support removing custom schema attributes (`family_id`, `family_role`) in-place via CloudFormation update. After removing the legacy `Schema` block (ADR-0011) and adding `X-Family-Id` to API Gateway CORS `AllowHeaders`, recreate the core stack cleanly:

```powershell
# 1. Delete existing core stack
sam delete --stack-name familyledger-core-dev --region us-east-1 --no-prompts

# 2. Deploy clean stack (generates fresh Cognito User Pool & App Client IDs)
sam deploy --config-file samconfig.toml
```

After recreating `familyledger-core-dev`, update `COGNITO_USER_POOL_ID`, `COGNITO_CLIENT_ID`, `COGNITO_ISSUER`, `API_GATEWAY_URL`, and `CLOUDFRONT_URL` in `backend/.env.dev` from the stack outputs.

---

## 2. Data & Event Stack (`familyledger-data-dev`)

### Deploy or Update (Zero Idle Cost)
```powershell
sam deploy --config-file samconfig-data.toml
```

### Delete the Data Stack
```powershell
sam delete --stack-name familyledger-data-dev --region us-east-1 --no-prompts
```

For full event-publishing architecture analysis and cost comparison, see:
[`documentation/architecture/kinesis-cdc-streaming.md`](../../../documentation/architecture/kinesis-cdc-streaming.md) and [`documentation/decisions/0017-cdc-and-event-publishing-strategies.md`](../../../documentation/decisions/0017-cdc-and-event-publishing-strategies.md).
