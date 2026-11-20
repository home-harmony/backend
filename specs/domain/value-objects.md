# VO — Core Domain Value Objects

- **Prefix**: `VO`
- **Layer**: `domain`
- **Unit under test**: `domain::value_objects::{Money, CurrencyCode, FamilyName}`
- **Test seam**: `Layer 1 (Pure Domain)`
- **Format**: `P` (Property & Boundary Invariants)
- **Test target**: `domain/tests/vo_value_objects.rs`

---

## Vocabulary

| Term | Meaning / API Symbol | Concrete Representation |
| :--- | :--- | :--- |
| `Money` | `domain::value_objects::Money` | Non-negative float-free amount (`rust_decimal::Decimal >= 0`) + `CurrencyCode`. Signed balances belong to `AccountBalanceSnapshot`, not `Money`. |
| `CurrencyCode` | `domain::value_objects::CurrencyCode` | ISO 4217 3-letter uppercase ASCII string |
| `FamilyName` | `domain::value_objects::FamilyName` | Validated non-empty trimmed string ($\le 200$ chars) |
| `DomainError` | `domain::errors::DomainError` | Enum variants (`CurrencyMismatch`, `NegativeAmount`, `InvalidCurrencyCode`) |

---

## Rules

### VO-01 — Money construction rejects negative amounts and accepts zero or positive decimals
- **Status**: `Draft`
- **Origin**: `adr:0005`
- **For all**: `amount: Decimal` and valid `currency: CurrencyCode`.
- **Then**: `Money::new(amount, currency)` returns `Ok(m)` with `m.amount() == amount` when `amount >= 0`, and `Err(DomainError::NegativeAmount { amount })` when `amount < 0`.
- **Oracle**: Sign comparison against `Decimal::ZERO`.
- **Boundary Examples**:
  - `0.00 USD` $\rightarrow$ `Ok` (`is_zero() == true`)
  - `0.01 USD` $\rightarrow$ `Ok`
  - `-0.01 USD` $\rightarrow$ `Err(DomainError::NegativeAmount { .. })`

### VO-02 — Money addition requires matching CurrencyCode
- **Status**: `Draft`
- **Origin**: `doc:documentation/domain/ubiquitous-language.md`
- **For all**: Two `Money` values `a(amt_a, cur_a)` and `b(amt_b, cur_b)`.
- **Then**: `a.add(&b)` returns `Ok(Money(amt_a + amt_b, cur_a))` iff `cur_a == cur_b`, and `Err(DomainError::CurrencyMismatch { expected: cur_a, actual: cur_b })` otherwise.
- **Oracle**: Exact decimal addition when ISO codes match; error otherwise.
- **Boundary Examples**:
  - `10.50 USD + 4.50 USD` $\rightarrow$ `Ok(15.00 USD)`
  - `0.00 USD + 0.00 EUR` $\rightarrow$ `Err(DomainError::CurrencyMismatch { .. })`

### VO-03 — Money subtraction requires matching CurrencyCode and non-negative result
- **Status**: `Draft`
- **Origin**: `human`
- **For all**: Two `Money` values `a(amt_a, cur_a)` and `b(amt_b, cur_b)`.
- **Then**:
  - If `cur_a != cur_b`, `a.sub(&b)` returns `Err(DomainError::CurrencyMismatch { expected: cur_a, actual: cur_b })`.
  - If `cur_a == cur_b` and `amt_a >= amt_b`, `a.sub(&b)` returns `Ok(Money(amt_a - amt_b, cur_a))`.
  - If `cur_a == cur_b` and `amt_a < amt_b`, `a.sub(&b)` returns `Err(DomainError::NegativeAmount { amount: amt_a - amt_b })` (`Money` is strictly zero or positive; negative balances belong to `AccountBalanceSnapshot`).
- **Oracle**: Exact decimal subtraction clamped to non-negative `Money` invariants.
- **Boundary Examples**:
  - `50.00 USD - 20.00 USD` $\rightarrow$ `Ok(30.00 USD)`
  - `50.00 USD - 50.00 USD` $\rightarrow$ `Ok(0.00 USD)`
  - `5.00 USD - 10.00 USD` $\rightarrow$ `Err(DomainError::NegativeAmount { amount: -5.00 })`
  - `50.00 USD - 10.00 MDL` $\rightarrow$ `Err(DomainError::CurrencyMismatch { .. })`

### VO-04 — CurrencyCode enforces 3 trimmed uppercase ASCII letters
- **Status**: `Draft`
- **Origin**: `observed`
- **For all**: Input `&str` `raw`.
- **Then**: `CurrencyCode::new(raw)` succeeds iff `raw.trim()` has length `3` and all bytes are ASCII `'A'..='Z'`; otherwise returns `Err(DomainError::InvalidCurrencyCode { .. })`.
- **Oracle**: ASCII uppercase length-3 predicate on trimmed input.
- **Boundary Examples**:
  - `"USD"`, `"  EUR  "` $\rightarrow$ `Ok`
  - `"usd"`, `"US"`, `"USDD"`, `""`, `"U$D"` $\rightarrow$ `Err(DomainError::InvalidCurrencyCode { .. })`

### VO-05 — FamilyName trims whitespace and enforces 1..=200 Unicode characters
- **Status**: `Draft`
- **Origin**: `doc:documentation/design-review-2026-08-28.md`
- **For all**: Input `&str` `raw`.
- **Then**: `FamilyName::try_new(raw)` returns `Ok` containing `raw.trim()` iff `1 <= trimmed.chars().count() <= 200`; otherwise returns `Err`.
- **Oracle**: Unicode char count of trimmed string in `[1, 200]`.
- **Boundary Examples**:
  - `"  Smith Family  "` $\rightarrow$ `Ok("Smith Family")`
  - `""`, `"   "` $\rightarrow$ `Err`
  - 200 `"a"` chars $\rightarrow$ `Ok`; 201 `"a"` chars $\rightarrow$ `Err`

---

## Open Questions

*(None)*

---

## Findings

| # | Location | Code Says | Doc / Spec Says | Impact |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `backend/domain/src/value_objects/money.rs` | Updated `Money::sub` to delegate through `Money::new` | `VO-03` (`Origin: human`): `Money` is strictly $\ge 0$; negative balances belong to `AccountBalanceSnapshot` | Resolved on 2026-11-20 |
