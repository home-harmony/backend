# Behavior Specifications Index

Declarative behavior specifications serving as the single source of truth for human-approved behavior and blind AI test generation (see [ADR-0019](../../documentation/decisions/0019-declarative-test-specifications.md)).

- **Operational Guidelines**: [`GUIDELINES.md`](./GUIDELINES.md)
- **Specification Template**: [`TEMPLATE.md`](./TEMPLATE.md)
- **Worked Examples**: [`examples.md`](./examples.md)
- **Agent Skill**: [`spec-driven-dev`](../../documentation/.ai/skills/spec-driven-dev/SKILL.md)

---

## Prefix Registry

Prefixes are 2–4 uppercase ASCII letters, globally unique across all specifications, and never reused.

| Prefix | Domain / Component | Layer | Format | Spec Document | Test Target (`backend/`) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `VO` | Value Objects (`Money`, `SignedMoney`, `PositiveMoney`, `CurrencyCode`, etc.) | `domain` | `P` (Property) | [`domain/value-objects.md`](./domain/value-objects.md) | `domain/tests/vo_value_objects.rs` |
| `FM` | Family Workspace & RBAC Role Permissions | `domain` | `T` (Table) | [`domain/family-rbac.md`](./domain/family-rbac.md) | `domain/tests/fm_family_rbac.rs` |
| `AC` | Payment Accounts, Derived Balances & Snapshots | `domain` | `T` (Table) | [`domain/payment-accounts.md`](./domain/payment-accounts.md) | `domain/tests/ac_payment_accounts.rs` |
| `LD` | Ledger Categories, Transactions, FX & Amendments | `domain` | `T` (Table) | [`domain/ledger-immutability.md`](./domain/ledger-immutability.md) | `domain/tests/ld_ledger_immutability.rs` |
| `LN` | Loans, Payment Splits & Debt Optimizers *(Planned)* | `domain` | `T` / `P` | `domain/loan-repayment.md` | `domain/tests/ln_loan_repayment.rs` |
| `BG` | Monthly Budget Draft & Owner Approval *(Planned)* | `api` | `U` (Use Case) | `api/budget-approval.md` | `api/tests/bg_budget_approval.rs` |
| `AU` | Cognito JWT & `X-Family-Id` Isolation | `api` | `U` (Use Case) | [`api/multi-family-auth.md`](./api/multi-family-auth.md) | `api/tests/au_multi_family_auth.rs` |
| `IDP` | Cognito `PreSignUp` & Google Account Linking | `api` | `T` (Table) | [`api/cognito-account-linking.md`](./api/cognito-account-linking.md) | `lambdas/cognito_pre_signup/tests/idp_account_linking.rs` |

---

## Automated Traceability & Governance Gate

Run from `backend/`:
```bash
cargo test -p domain --test spec_traceability
```

Because `backend/specs/` lives directly inside the `backend` repository, CI runs this gate self-contained with zero cross-repo dependencies and enforces four checks:
1. **Valid Metadata**: Every rule in `specs/domain/` and `specs/api/` has a unique `<PREFIX>-<NN>` ID, a valid `Status` (`Draft`, `Approved`, `Deprecated`), and a valid `Origin` (`human`, `observed`, `adr:...`, `doc:...`).
2. **Block Premature Tests**: Any test referencing a `Draft` or `Deprecated` rule fails CI immediately (`only 'Approved' rules may have spec tests`).
3. **Block Unconfirmed Approvals**: Any `Approved` rule that still carries `Origin: observed` or is still referenced under `## Open Questions` fails CI immediately.
4. **1:1 Test Coverage**: Every `Approved` rule ID is referenced in `backend/{domain,infrastructure,api,lambdas/*}/tests/*.rs`, and every test rule ID exists in `backend/specs/`.
