# FamilyLedger — AWS SAM Infrastructure Stacks

This directory contains Infrastructure-as-Code (IaC) templates for AWS SAM following the **Option B2 3-Stack 1-Way DAG** (`services` $\rightarrow$ `core` & `data`):

| Stack Name | Template File | Config File | Description | Idle Cost |
| :--- | :--- | :--- | :--- | :--- |
| `familyledger-core-dev` | `template.yaml` | `samconfig.toml` | Cognito User Pool (identity only; tenant context via `X-Family-Id`), Web Client, S3 SPA Bucket, CloudFront CDN | **$0.00** |
| `familyledger-data-dev` | `data-stream-template.yaml` | `samconfig-data.toml` | Custom EventBridge Event Bus (`familyledger-events-dev`), least-privilege IAM Execution Roles (`FamilyLedgerServiceRole`, `FamilyLedgerMigrationRunnerRole`), and `FamilyLedgerMigrateRunnerFunction` (`familyledger-migrate-dev`) | **$0.00** |
| `familyledger-services-dev` | `services-template.yaml` | `samconfig-services.toml` | API Gateway HTTP API (`CognitoJwtAuthorizer` + `X-Family-Id` CORS) and stateless Rust HTTP Lambdas (`family_service` with `DSQL_USER=familyledger_app`, importing exports from `core` and `data`) | **$0.00** |

> [!NOTE]
> **Event Publishing Architecture (ADR-0017)**: Amazon Kinesis Data Stream is disabled/removed due to fixed idle hourly fees ($10.80–$28.80/month). Rust service Lambdas publish domain events directly to the EventBridge bus (`events:PutEvents`) at **$0.00 idle cost**.

---

## 1. Core Stack (`familyledger-core-dev`) — Recreating Cleanly

Amazon Cognito does not support removing custom schema attributes (`family_id`, `family_role`) in-place via CloudFormation update. After removing the legacy `Schema` block (ADR-0011) and moving `FamilyLedgerApi` into `services-template.yaml`, update or recreate the core stack cleanly:

```powershell
# 1. Delete existing core stack (only if recreating Cognito User Pool cleanly)
sam delete --stack-name familyledger-core-dev --region us-east-1 --no-prompts

# 2. Deploy clean stack (generates Cognito User Pool, Web Client, S3 SPA Bucket, CloudFront)
sam deploy --template-file template.yaml --config-file samconfig.toml
```

After recreating `familyledger-core-dev`, update `COGNITO_USER_POOL_ID`, `COGNITO_CLIENT_ID`, `COGNITO_ISSUER`, and `CLOUDFRONT_URL` in `backend/.env.dev` from the stack outputs (`API_GATEWAY_URL` is exported by `familyledger-services-dev`).

---

## 2. Data, Event & Migration Stack (`familyledger-data-dev`)

### Build `migrate_runner`, Deploy & Invoke (Zero Idle Cost)
*(Ensure `cargo-lambda >= 1.9.2` and `zig >= 0.17.1` are installed via `scoop update cargo-lambda zig`.)*
```powershell
# 1. From backend/: build the ARM64 Lambda binary (outputs target/lambda/migrate_runner/bootstrap)
cargo lambda build --release --arm64 --package migrate_runner

# 2. From backend/infrastructure/sam/: validate and deploy
sam validate -t data-stream-template.yaml --lint
sam deploy --template-file data-stream-template.yaml --config-file samconfig-data.toml --capabilities CAPABILITY_NAMED_IAM --no-confirm-changeset

# 3. Invoke the migration runner against Aurora DSQL & verify report
aws lambda invoke --function-name familyledger-migrate-dev --region us-east-1 --log-type Tail --cli-binary-format raw-in-base64-out --payload '{}' target\migrate-response.json
Get-Content target\migrate-response.json
Remove-Item target\migrate-response.json -Force
```

> [!NOTE]
> **Runtime Database Role (`DSQL_USER`)**: Services running under `FamilyLedgerServiceRole` (`dsql:DbConnect` without `dsql:DbConnectAdmin`) must set `DSQL_USER` to an IAM-mapped non-admin DSQL role (see [`aws-deployment-and-teardown.md`](../../../documentation/architecture/aws-deployment-and-teardown.md)).

### Delete the Services & Data Stacks
*(Always delete `familyledger-services-dev` before `familyledger-data-dev` and `familyledger-core-dev` because `services` imports CloudFormation exports from both.)*
```powershell
sam delete --stack-name familyledger-services-dev --region us-east-1 --no-prompts
sam delete --stack-name familyledger-data-dev --region us-east-1 --no-prompts
```

---

## 3. Services Stack (`familyledger-services-dev`)

Deploy `familyledger-services-dev` **after** `familyledger-core-dev` (with legacy `FamilyLedgerApi` removed from `template.yaml`), `familyledger-data-dev`, `migrate_runner` invocation (`36` migrations), and the `familyledger_app` Aurora DSQL role bootstrap:

```powershell
# 1. From backend/: build the ARM64 family_service Lambda binary (outputs target/lambda/family_service/bootstrap)
cargo lambda build --release --arm64 --package family_service

# 2. From backend/infrastructure/sam/: validate and deploy (exports ApiGatewayUrl)
sam validate -t services-template.yaml --lint
sam deploy --template-file services-template.yaml --config-file samconfig-services.toml --no-confirm-changeset
```

> [!NOTE]
> **Services stack parameters**: `DsqlUser` (default `familyledger_app`; the `AllowedPattern` rejects `admin`) sets the Lambda's `DSQL_USER`. The `http://localhost:4200` CORS origin is added **only when `Environment=dev`** (`IsDev` condition); `staging`/`prod` allow only the CloudFront origin. Core `template.yaml` keeps its localhost Cognito callback by design.

> [!WARNING]
> **Scrub before publishing**: `samconfig-data.toml` and `samconfig-services.toml` contain the dev DSQL cluster ID (matching the Sprint 1 convention for this private repo). Replace it with a placeholder before making the repository public.

---

## 4. AWS Deployment & Teardown Runbooks

- **Sprint 2 Upgrade & Live Verification Guide (`dev`)**: [`documentation/roadmap/sprint-2/aws-deployment-guide.md`](../../../documentation/roadmap/sprint-2/aws-deployment-guide.md) — step-by-step upgrade from Sprint 1, `familyledger_app` role bootstrap, and live Cognito `IdToken` + `X-Family-Id` smoke tests.
- **Overall Product AWS Deployment, Operations & Teardown Guide**: [`documentation/architecture/aws-deployment-and-teardown.md`](../../../documentation/architecture/aws-deployment-and-teardown.md) — full Option B2 bootstrap across environments, day-2 operations, clean recreation, and complete teardown.
