# AC — Payment Accounts, Derived Balances & Monthly Snapshots

- **Prefix**: `AC`
- **Layer**: `domain`
- **Unit under test**: `domain::entities::{PaymentAccount, AccountKind, AccountBalanceSnapshot}` and balance derivation calculator
- **Test seam**: `Layer 1 (Pure Domain)`
- **Format**: `T` (Case Table)
- **Test target**: `domain/tests/ac_payment_accounts.rs`

---

## Vocabulary

| Term | Meaning / API Symbol | Concrete Representation |
| :--- | :--- | :--- |
| `PaymentAccount` | `domain::entities::PaymentAccount` | Aggregate root representing a family payment instrument (`AccountId`, `FamilyId`, `owner_member_id: MemberId`, `AccountName`, `AccountKind`, `CurrencyCode`, `opening_balance: SignedMoney`, `credit_limit: Option<Money>`, `last_four`, `bank_name`, `color`, `included_in_budget`, `created_at`, `deleted_at`) |
| `AccountKind` | `domain::entities::AccountKind` | `CreditCard`, `DebitCard`, `Cash`, `BankAccount` |
| `AccountBalanceSnapshot` | `domain::entities::AccountBalanceSnapshot` | Immutable month-end closing balance projection (`SnapshotId` UUID v7, `AccountId`, `FamilyId`, `YearMonth`, `closing_balance: SignedMoney`, `computed_at`) |
| `LedgerMovement` | `domain::entities::LedgerMovement` (or balance input slice) | Pure domain projection of an active (`deleted_at == None`) transaction leg (`occurred_at`, `source_account_id`, `destination_account_id`, `amount: PositiveMoney`, `destination_amount: Option<PositiveMoney>`) |
| `DomainError` | `domain::errors::DomainError` | Typed domain errors (`CreditLimitNotAllowed`, `CurrencyMismatch`, `AccountDeleted`, `AccountNotFound`, `InsufficientRole`, `InvariantViolation`) |

---

## Rules

| ID | Status | Origin | Given State / Caller & Inputs | Action / Input | Expected Result / Next State |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `AC-01` | `Draft` | `human` | Caller with `ManageOwnAccounts` (`Owner`, `Member`, `Child`, or `Guest([ManageOwnAccounts])`), `kind = CreditCard`, `currency = USD`, `opening_balance = -250.00 USD`, `credit_limit = Some(5000.00 USD)` | `PaymentAccount::register(...)` | Succeeds, assigns `AccountId` (UUID v4), sets `opening_balance == -250.00 USD` and `credit_limit == Some(5000.00 USD)`, emits `[DomainEvent::AccountRegistered]` |
| `AC-02` | `Draft` | `doc:documentation/architecture/database-schema.md` | Non-`CreditCard` kind (`DebitCard`, `Cash`, or `BankAccount`) with `credit_limit = Some(1000.00 USD)` | `PaymentAccount::register(...)` or `update_details(...)` | `Err(DomainError::CreditLimitNotAllowed { kind })` |
| `AC-03` | `Draft` | `adr:0005` | `opening_balance` or `credit_limit` whose `CurrencyCode` (`EUR`) differs from `account.currency` (`USD`) | `PaymentAccount::register(...)` or `update_details(...)` | `Err(DomainError::CurrencyMismatch { expected: USD, actual: EUR })` |
| `AC-04` | `Draft` | `human` | Non-`Owner` caller (`Member`, `Child`, or `Guest([ManageOwnAccounts])`) attempting to register an account with `owner_member_id != caller_member_id` | `PaymentAccount::register(...)` | `Err(DomainError::InsufficientRole { role })` (`Role::Owner` with `owner_member_id != caller_member_id` succeeds) |
| `AC-05` | `Draft` | `human` | Active `PaymentAccount` owned by `M1` | `update_details` by `M1` (with `ManageOwnAccounts`) or by Family `Role::Owner` (`M_owner`) | Updates mutable fields (`name`, `credit_limit`, `last_four`, `bank_name`, `color`, `included_in_budget`) while preserving `id`, `owner_member_id`, `kind`, `currency`, `opening_balance`, and `created_at`; emits `0` events |
| `AC-06` | `Draft` | `human` | Active `PaymentAccount` owned by `M1`; caller is `M2 != M1` with `Role::Member`, `Role::Child`, or `Role::Guest([ViewAllAccounts, ManageOwnAccounts])` | `update_details(...)` or `soft_delete(...)` | `Err(DomainError::InsufficientRole { role })` (only account owner with `ManageOwnAccounts` or Family `Role::Owner` may mutate/delete) |
| `AC-07` | `Draft` | `human` | Soft-deleted `PaymentAccount` (`deleted_at = Some(t)`) | `update_details(...)` or `soft_delete(...)` | `Err(DomainError::AccountDeleted { account_id })` |
| `AC-08` | `Draft` | `human` | Active `PaymentAccount` owned by `M1`; caller is `Role::Child` (`M2 != M1`) or `Role::Guest([ManageOwnAccounts])` without `ViewAllAccounts` | `can_view_account(caller_member_id, role, &perms)` | `false` (`Err(DomainError::InsufficientRole { role })`); `true` when `M1 == caller_member_id` or caller has `ViewAllAccounts` |
| `AC-09` | `Draft` | `adr:0006` | Account (`USD`, `opening_balance = 1000.00 USD`), `latest_snapshot = None`, active movements: `Income(+500.00 USD)`, `Expense(-120.25 USD)`, `LoanPayment(-200.00 USD)`, same-currency `Transfer` out (`-100.00 USD`), `CashWithdrawal` out (`-50.00 USD`) | `derive_current_balance(&account, None, &movements)` | `Ok(SignedMoney(1029.75 USD))` |
| `AC-10` | `Draft` | `adr:0015` | Source account `A` (`MDL`, `opening = 5000.00 MDL`) and destination account `B` (`EUR`, `opening = 0.00 EUR`); active `CrossCurrencyTransfer` (`source = A`, `dest = B`, `amount = 1950.00 MDL`, `destination_amount = Some(100.00 EUR)`) | `derive_current_balance` for `A` and `B` | `A` equals `3050.00 MDL` (`-amount`); `B` equals `100.00 EUR` (`+destination_amount`) |
| `AC-11` | `Draft` | `adr:0012` | Accounts `A` (`MDL`, `opening = 5000.00 MDL`) and `B` (`EUR`, `opening = 0.00 EUR`) with superseded `CrossCurrencyTransfer` (`original.deleted_at == None`, `-1950 MDL` / `+100 EUR`), its `Reversal` (`source = B`, `dest = A`, `amount = 100 EUR`, `destination_amount = Some(1950 MDL)`), and `Corrected` (`-1960 MDL` / `+100 EUR`) | `derive_current_balance` for `A` and `B` | Original + `Reversal` cancel to `0` in both currencies (`A = 3040.00 MDL`, `B = 100.00 EUR`) with zero currency mismatch or double-negation |
| `AC-12` | `Draft` | `adr:0006` | Account (`USD`, `opening = 500.00 USD`), `latest_snapshot = Snapshot(year_month = "2026-10", closing_balance = 800.00 USD)`, movements at `2026-10-31T23:59:59Z` (`-50 USD`, inside snapshot) and `2026-11-01T00:00:00Z` (`+125 USD`, after snapshot) | `derive_current_balance(&account, Some(&snapshot), &movements)` | Ignores `<= 2026-10-31T23:59:59.999999Z` movement and `opening_balance` (already folded into `800.00 USD`); returns `Ok(SignedMoney(925.00 USD))` |
| `AC-13` | `Draft` | `adr:0012` | Account with closed `Snapshot("2026-10", closing = 800.00 USD)` where an October `Expense(-100.00 USD)` is amended on `2026-11-05` (`Reversal(+100.00 USD)` + `Corrected(-60.00 USD)` recorded with `occurred_at > 2026-10-31T23:59:59.999999Z`, `original.deleted_at == None`) | `derive_current_balance(&account, Some(&snapshot), &post_oct_movements)` | Returns `Ok(SignedMoney(840.00 USD))` (`800 + 100 - 60`) without mutating the closed `"2026-10"` snapshot |
| `AC-14` | `Draft` | `adr:0006` | Account (`EUR`, `opening = -100.00 EUR`), `prior_snapshot = None`, active movements through `2026-11-30T23:59:59.999999Z` summing to `-50.00 EUR` (plus a `2026-12-01` movement of `+300.00 EUR`) | `compute_month_end_snapshot(&account, None, YearMonth("2026-11"), &movements, now)` | Produces `AccountBalanceSnapshot` (`SnapshotId` UUID v7, `closing_balance == -150.00 EUR`) folding in `opening_balance` and excluding December movements, plus `[DomainEvent::AccountBalanceSnapshotted]` |

---

## Open Questions

*(None)*

---

## Findings

*(None — initial Sprint 3 Phase 0 draft)*
