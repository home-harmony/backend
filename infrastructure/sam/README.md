# FamilyLedger — AWS SAM Infrastructure Stacks

This directory contains Infrastructure-as-Code (IaC) templates for AWS SAM:

| Stack Name | Template File | Config File | Description | Idle Cost |
| :--- | :--- | :--- | :--- | :--- |
| `familyledger-core-dev` | `template.yaml` | `samconfig.toml` | Cognito User Pool, Web Client, S3 SPA Bucket, CloudFront CDN, API Gateway HTTP API | **$0.00** |
| `familyledger-data-dev` | `data-stream-template.yaml` | `samconfig-data.toml` | EventBridge Event Bus, IAM Execution Roles, and Kinesis CDC Stream (optional) | **$0.00** (with `DeployKinesisStream="false"`) |

---

## 1. Quick Commands: Delete & Re-Create `familyledger-data-dev`

### To Delete the Data Stack (Stop all Kinesis charges):
```powershell
sam delete --stack-name familyledger-data-dev --region us-east-1 --no-prompts
```

### To Re-Create Without Kinesis ($0.00 Idle Cost — Recommended for Sprints 1–3):
```powershell
sam deploy --config-file samconfig-data.toml
```
*(Uses defaults in `samconfig-data.toml`: `DeployKinesisStream="false"`)*

### To Re-Create With 1 Provisioned Shard ($0.36/Day — For CDC Testing):
```powershell
sam deploy `
  --config-file samconfig-data.toml `
  --parameter-overrides `
    Environment="dev" `
    DsqlClusterIdentifier="4bublzlvvtnwjpmgtotwrulv7m" `
    DeployKinesisStream="true" `
    KinesisCapacityMode="PROVISIONED" `
    KinesisShardCount=1
```

### To Re-Create With On-Demand Mode ($0.96/Day — For Production):
```powershell
sam deploy `
  --config-file samconfig-data.toml `
  --parameter-overrides `
    Environment="prod" `
    DsqlClusterIdentifier="<prod-cluster-id>" `
    DeployKinesisStream="true" `
    KinesisCapacityMode="ON_DEMAND"
```

---

## 2. Kinesis Modes & Pricing Summary

* **`DeployKinesisStream="false"`**: **$0.00 / month**. Only deploys IAM roles & EventBridge.
* **`PROVISIONED` (1 shard)**: **$0.015 / hour** (~$0.36/day, ~$10.80/month). Up to 1 MB/s write (1,000 records/sec).
* **`ON_DEMAND`**: **$0.040 / hour** (~$0.96/day, ~$28.80/month). Auto-scales up to 200 MB/s write.

For detailed architecture analysis and cost formulas, see:
[`documentation/architecture/kinesis-cdc-streaming.md`](../../../documentation/architecture/kinesis-cdc-streaming.md)

