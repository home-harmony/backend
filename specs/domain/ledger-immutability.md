# LD — Ledger Categories, Transactions, Cross-Currency Transfers & Amendments

- **Prefix**: `LD`
- **Layer**: `domain`
- **Unit under test**: `domain::entities::{Category, CategoryKind, Transaction, TransactionKind}` and amendment / FX spread logic
- **Test seam**: `Layer 1 (Pure Domain)`
- **Format**: `T` (Case Table)
- **Test target**: `domain/tests/ld_ledger_immutability.rs`

---

## Vocabulary

| Term | Meaning / API Symbol | Concrete Representation |
| :--- | :--- | :--- |
| `Category` | `domain::entities::Category` | Ledger category (`CategoryId`, `family_id: Option<FamilyId>`, `CategoryName`, `CategoryKind`, `icon: Option<CategoryIcon>`, `color: Option<HexColor>`, `deleted_at`) |
| `CategoryKind` | `domain::entities::CategoryKind` | `Income`, `Expense`, `Transfer` |
| `Transaction` | `domain::entities::Transaction` | Immutable financial movement (`TransactionId` UUID v7, `FamilyId`, `recorded_by: MemberId`, `TransactionKind`, `amount: PositiveMoney`, `destination_amount: Option<PositiveMoney>`, `source_account_id`, `destination_account_id`, `category_id`, `amendment_of_id`, `tags`, `description`, `occurred_at`, `created_at`, `idempotency_key: Uuid`, `deleted_at`) |
| `TransactionKind` | `domain::entities::TransactionKind` | `Income`, `Expense`, `Transfer`, `CashWithdrawal`, `LoanPayment`, `Reversal` |
| `AccountContext` | `domain::ports::AccountAccess` | Verified target account metadata (`account_id`, `owner_member_id`, `kind: AccountKind`, `currency: CurrencyCode`, `is_deleted: bool`) |
| `DomainError` | `domain::errors::DomainError` | Typed domain errors (`CannotModifySystemCategory`, `CategoryDeleted`, `CategoryKindMismatch`, `AccountDeleted`, `CurrencyMismatch`, `SameAccountTransfer`, `InvalidCashWithdrawalAccounts`, `CrossCurrencyDestinationRequired`, `SameCurrencyDestinationForbidden`, `CannotAmendReversal`, `TransactionAlreadyAmended`, `TransactionDeleted`, `InsufficientRole`, `IdempotencyConflict`) |

---

## Rules

| ID | Status | Origin | Given State / Accounts & Caller | Action / Input | Expected Result / Next State |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `LD-01` | `Draft` | `human` | Global system `Category` (`family_id == None`) | `update(...)` or `soft_delete(...)` by any role (including `Role::Owner`) | `Err(DomainError::CannotModifySystemCategory { category_id })` |
| `LD-02` | `Draft` | `human` | Family custom `Category` (`family_id == Some(F)`); caller has `RecordAnyTransaction` vs. caller lacks `RecordAnyTransaction` (`Child` or `Guest([RecordOwnTransactions])`) | `Category::create_custom` / `update` / `soft_delete` | Succeeds for `RecordAnyTransaction`; returns `Err(DomainError::InsufficientRole { role })` without `RecordAnyTransaction` |
| `LD-03` | `Draft` | `doc:documentation/domain/ubiquitous-language.md` | Active destination account `A` (`USD`), active `Category` (`CategoryKind::Income`), `amount = 2500.00 USD` | `Transaction::record_income(...)` | Succeeds with `TransactionId` (UUID v7), `source_account_id == None`, `destination_account_id == Some(A)`, `destination_amount == None`, emits `[DomainEvent::TransactionRecorded]` |
| `LD-04` | `Draft` | `doc:documentation/domain/ubiquitous-language.md` | Active source account `A` (`USD`), active `Category` (`CategoryKind::Expense`), `amount = 45.50 USD` | `Transaction::record_expense(...)` | Succeeds with `source_account_id == Some(A)`, `destination_account_id == None`, `destination_amount == None`, emits `[DomainEvent::TransactionRecorded]` |
| `LD-05` | `Draft` | `human` | `Income` with `CategoryKind::Expense` (or `None`), `Expense` with `CategoryKind::Income` (or `None`), or `Transfer`/`CashWithdrawal` with `Some(CategoryKind::Expense)` | `Transaction::record_*` | `Err(DomainError::CategoryKindMismatch { .. })` (`Transfer`/`CashWithdrawal` accept `None` or `Some(CategoryKind::Transfer)`) |
| `LD-06` | `Draft` | `doc:documentation/domain/ubiquitous-language.md` | `Transfer` or `CashWithdrawal` where `source_account_id == destination_account_id` | `Transaction::record_transfer` / `record_cash_withdrawal` | `Err(DomainError::SameAccountTransfer { account_id })` |
| `LD-07` | `Draft` | `human` | `CashWithdrawal` where `source_account.kind == Cash` OR `destination_account.kind != Cash` | `Transaction::record_cash_withdrawal(...)` | `Err(DomainError::InvalidCashWithdrawalAccounts)` (`source` must be `CreditCard \| DebitCard \| BankAccount` and `destination` must be `Cash`) |
| `LD-08` | `Draft` | `adr:0015` | Same-currency `Transfer` or `CashWithdrawal` (`source = USD`, `dest = USD`) with `destination_amount = Some(50.00 USD)` | `Transaction::record_transfer` / `record_cash_withdrawal` | `Err(DomainError::SameCurrencyDestinationForbidden)` |
| `LD-09` | `Draft` | `adr:0015` | Cross-currency `Transfer` or `CashWithdrawal` (`source = MDL`, `dest = EUR`) with `destination_amount = None` OR `destination_amount = Some(100.00 USD)` | `Transaction::record_transfer` / `record_cash_withdrawal` | `Err(DomainError::CrossCurrencyDestinationRequired)` when `None`; `Err(DomainError::CurrencyMismatch { expected: EUR, actual: USD })` when currency mismatches destination |
| `LD-10` | `Draft` | `adr:0015` | Valid cross-currency `Transfer` (`amount = 1950.00 MDL`, `destination_amount = Some(100.00 EUR)`), family `home_currency = MDL` | `tx.implied_rate()` and `tx.fx_spread(MDL, rates)` | `implied_rate() == Some(100 / 1950)`; `fx_spread` returns `Some(SignedMoney(30.00 MDL))` when `1 EUR = 19.20 MDL` rate exists (`1950 - 1920`), and `None` when required rate is absent |
| `LD-11` | `Draft` | `human` | Referenced `source_account`, `destination_account`, or `category` has `is_deleted == true` | `Transaction::record_*` or `amend(...)` | `Err(DomainError::AccountDeleted { account_id })` or `Err(DomainError::CategoryDeleted { category_id })` |
| `LD-12` | `Draft` | `human` | Caller has `RecordOwnTransactions` without `RecordAnyTransaction` (`Role::Child` or `Role::Guest([RecordOwnTransactions])`) and references an account where `owner_member_id != caller_member_id` | `Transaction::record_*` | `Err(DomainError::InsufficientRole { role })` (succeeds when all referenced accounts have `owner_member_id == caller_member_id`) |
| `LD-13` | `Draft` | `adr:0012` | Active un-amended single-currency `Expense` (`original`: `source = A`, `amount = 50.00 USD`, `deleted_at = None`); valid corrected payload (`amount = 65.00 USD`) and client `idempotency_key = K` | `Transaction::amend(&original, false, corrected_input, K, now)` | Keeps `original.deleted_at == None`; creates `Reversal` (`kind = Reversal`, `source = None`, `destination = Some(A)`, `amount = 50.00 USD`, `amendment_of_id = Some(original.id)`, deterministic derived `idempotency_key != K`) and `Corrected` (`amendment_of_id = Some(original.id)`, `idempotency_key == K`, `Reversal.id < Corrected.id`); emits `[TransactionReversed, TransactionRecorded]` |
| `LD-14` | `Draft` | `adr:0012` | Active un-amended `CrossCurrencyTransfer` (`source = A(MDL)`, `dest = B(EUR)`, `amount = 1950.00 MDL`, `destination_amount = Some(100.00 EUR)`) | `Transaction::amend(...)` | `Reversal` sets `source = Some(B)`, `destination = Some(A)`, `amount = 100.00 EUR`, `destination_amount = Some(1950.00 MDL)`, preserving native account currencies on both legs |
| `LD-15` | `Draft` | `adr:0012` | Target transaction is `TransactionKind::Reversal`, already superseded (`is_already_amended == true`), or soft-deleted (`deleted_at.is_some()`) | `Transaction::amend(...)` | `Err(DomainError::CannotAmendReversal)` for `Reversal`; `Err(DomainError::TransactionAlreadyAmended { transaction_id })` if already amended; `Err(DomainError::TransactionDeleted { transaction_id })` if soft-deleted |
| `LD-16` | `Draft` | `adr:0012` | Active un-amended non-`Reversal` transaction (`deleted_at == None`) vs. `Reversal` / already-amended / already-deleted transaction | `Transaction::soft_delete(...)` | Sets `deleted_at = Some(now)` and emits `[DomainEvent::TransactionDeleted]` for active un-amended row; rejects `Reversal` (`CannotAmendReversal`), already-amended (`TransactionAlreadyAmended`), or already-deleted (`TransactionDeleted`) |
| `LD-17` | `Draft` | `human` | Existing transaction recorded by `M1`; caller `M2 != M1` has `RecordOwnTransactions` only (no `RecordAnyTransaction`, not `Role::Owner`) | `Transaction::amend(...)` or `soft_delete(...)` | `Err(DomainError::InsufficientRole { role })` (`Role::Owner` or any caller with `RecordAnyTransaction` succeeds) |
| `LD-18` | `Draft` | `human` | Same `(family_id, idempotency_key)` submitted twice with identical normalized payload vs. modified amount/account payload | Idempotency check & `derive_reversal_idempotency_key(family_id, key)` | Matching payload is recognized as idempotent replay (`0` new events); modified payload returns `Err(DomainError::IdempotencyConflict { idempotency_key })`; `derive_reversal_idempotency_key` is deterministic, distinct from `key`, and domain-separated by `family_id` |

---

## Open Questions

*(None)*

---

## Findings

*(None — initial Sprint 3 Phase 0 draft)*
