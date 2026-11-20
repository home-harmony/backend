# VO — Core Domain Value Objects

- **Prefix**: `VO`
- **Layer**: `domain`
- **Unit under test**: `domain::value_objects::{Money, CurrencyCode, FamilyName, DisplayName, Email, Relationship, InviteTtl}`
- **Test seam**: `Layer 1 (Pure Domain)`
- **Format**: `P` (Property & Boundary Invariants)
- **Test target**: `domain/tests/vo_value_objects.rs`

---

## Vocabulary

| Term | Meaning / API Symbol | Concrete Representation |
| :--- | :--- | :--- |
| `Money` | `domain::value_objects::Money` | Non-negative float-free amount (`rust_decimal::Decimal >= 0`) + `CurrencyCode`. Signed balances belong to `SignedMoney` / `AccountBalanceSnapshot`, not `Money`. |
| `CurrencyCode` | `domain::value_objects::CurrencyCode` | ISO 4217 3-letter uppercase ASCII string (`CurrencyCode::try_new`) |
| `FamilyName` | `domain::value_objects::FamilyName` | Validated non-empty trimmed string ($\le 200$ Unicode chars) |
| `DisplayName` | `domain::value_objects::DisplayName` | Validated non-empty trimmed string ($\le 100$ Unicode chars) |
| `Email` | `domain::value_objects::Email` | Trimmed, lowercased email address (`<= 254` bytes, unquoted local part, required TLD, no display text or domain literal) |
| `Relationship` | `domain::value_objects::Relationship` | Validated non-empty trimmed relationship label ($\le 50$ Unicode chars) |
| `InviteTtl` | `domain::value_objects::InviteTtl` | Bounded positive expiration duration in seconds (`1..=2_592_000`, max 30 days) |
| `DomainError` | `domain::errors::DomainError` | Enum variants (`CurrencyMismatch`, `NegativeAmount`, `ArithmeticOverflow`, `InvalidCurrencyCode`, `InvalidFamilyName`, `InvalidDisplayName`, `InvalidEmail`, `InvalidRelationship`, `InvalidInviteTtl`) |

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

### VO-02 — Money addition requires matching CurrencyCode and detects overflow
- **Status**: `Draft`
- **Origin**: `doc:documentation/domain/ubiquitous-language.md`
- **For all**: Two `Money` values `a(amt_a, cur_a)` and `b(amt_b, cur_b)`.
- **Then**:
  - If `cur_a != cur_b`, `a.add(&b)` returns `Err(DomainError::CurrencyMismatch { expected: cur_a, actual: cur_b })`.
  - If `cur_a == cur_b` and `amt_a + amt_b` overflows `Decimal`, returns `Err(DomainError::ArithmeticOverflow { operation: "money addition" })`.
  - Otherwise returns `Ok(Money(amt_a + amt_b, cur_a))`.
- **Oracle**: Checked exact decimal addition when ISO codes match.
- **Boundary Examples**:
  - `10.50 USD + 4.50 USD` $\rightarrow$ `Ok(15.00 USD)`
  - `0.00 USD + 0.00 EUR` $\rightarrow$ `Err(DomainError::CurrencyMismatch { .. })`
  - `Decimal::MAX USD + 1 USD` $\rightarrow$ `Err(DomainError::ArithmeticOverflow { .. })`

### VO-03 — Money subtraction requires matching CurrencyCode and non-negative result
- **Status**: `Draft`
- **Origin**: `human`
- **For all**: Two `Money` values `a(amt_a, cur_a)` and `b(amt_b, cur_b)`.
- **Then**:
  - If `cur_a != cur_b`, `a.sub(&b)` returns `Err(DomainError::CurrencyMismatch { expected: cur_a, actual: cur_b })`.
  - If `cur_a == cur_b` and `amt_a >= amt_b`, `a.sub(&b)` returns `Ok(Money(amt_a - amt_b, cur_a))`.
  - If `cur_a == cur_b` and `amt_a < amt_b`, `a.sub(&b)` returns `Err(DomainError::NegativeAmount { amount: amt_a - amt_b })` (`Money` is strictly zero or positive; signed balances belong to `SignedMoney` / `AccountBalanceSnapshot`).
- **Oracle**: Checked decimal subtraction clamped to non-negative `Money` invariants.
- **Boundary Examples**:
  - `50.00 USD - 20.00 USD` $\rightarrow$ `Ok(30.00 USD)`
  - `50.00 USD - 50.00 USD` $\rightarrow$ `Ok(0.00 USD)`
  - `5.00 USD - 10.00 USD` $\rightarrow$ `Err(DomainError::NegativeAmount { amount: -5.00 })`
  - `50.00 USD - 10.00 MDL` $\rightarrow$ `Err(DomainError::CurrencyMismatch { .. })`

### VO-04 — CurrencyCode enforces 3 trimmed uppercase ASCII letters
- **Status**: `Draft`
- **Origin**: `observed`
- **For all**: Input `&str` `raw`.
- **Then**: `CurrencyCode::try_new(raw)` succeeds iff `raw.trim()` has length `3` and all bytes are ASCII `'A'..='Z'`; otherwise returns `Err(CurrencyCodeError::PredicateViolated)` (which converts into `DomainError::InvalidCurrencyCode { .. }`).
- **Oracle**: ASCII uppercase length-3 predicate on trimmed input.
- **Boundary Examples**:
  - `"USD"`, `"  EUR  "` $\rightarrow$ `Ok`
  - `"usd"`, `"US"`, `"USDD"`, `""`, `"U$D"` $\rightarrow$ `Err`

### VO-05 — FamilyName trims whitespace and enforces 1..=200 Unicode characters
- **Status**: `Draft`
- **Origin**: `doc:documentation/design-review-2026-08-28.md`
- **For all**: Input `&str` `raw`.
- **Then**: `FamilyName::try_new(raw)` returns `Ok` containing `raw.trim()` iff `1 <= trimmed.chars().count() <= 200`; otherwise returns `Err(FamilyNameError::NotEmptyViolated)` or `Err(FamilyNameError::LenCharMaxViolated)` (which convert into `DomainError::InvalidFamilyName { .. }`).
- **Oracle**: Unicode char count of trimmed string in `[1, 200]`.
- **Boundary Examples**:
  - `"  Smith Family  "` $\rightarrow$ `Ok("Smith Family")`
  - `""`, `"   "` $\rightarrow$ `Err(FamilyNameError::NotEmptyViolated)`
  - 200 `"a"` chars $\rightarrow$ `Ok`; 201 `"a"` chars $\rightarrow$ `Err(FamilyNameError::LenCharMaxViolated)`

### VO-06 — Money multiplication and deserialization preserve non-negative and overflow invariants
- **Status**: `Draft`
- **Origin**: `human`
- **For all**: `Money` `m` and `factor: Decimal`, or JSON payload `{"amount": ..., "currency": ...}`.
- **Then**:
  - `m.multiply(factor)` returns `Err(DomainError::NegativeAmount { amount: factor })` if `factor < 0`, `Err(DomainError::ArithmeticOverflow { operation: "money multiplication" })` on overflow, and `Ok(Money(m.amount() * factor, m.currency().clone()))` otherwise.
  - Deserializing JSON with `amount < 0` or invalid `currency` fails.
- **Oracle**: Factor sign check + checked decimal multiplication; serde validation through `Money::new`.
- **Boundary Examples**:
  - `Money(1000.00 USD).multiply(1.05)` $\rightarrow$ `Ok(1050.00 USD)`
  - `Money(0.00 USD).multiply(-1)` $\rightarrow$ `Err(DomainError::NegativeAmount { .. })`
  - `serde_json::from_str::<Money>(r#"{"amount":"-1.00","currency":"USD"}"#)` $\rightarrow$ `Err`

### VO-07 — DisplayName trims whitespace and enforces 1..=100 Unicode characters
- **Status**: `Draft`
- **Origin**: `doc:documentation/architecture/database-schema.md`
- **For all**: Input `&str` `raw`.
- **Then**: `DisplayName::try_new(raw)` returns `Ok` containing `raw.trim()` iff `1 <= trimmed.chars().count() <= 100`; otherwise returns `Err(DisplayNameError::NotEmptyViolated)` or `Err(DisplayNameError::LenCharMaxViolated)` (`DomainError::InvalidDisplayName`).
- **Oracle**: Unicode char count of trimmed string in `[1, 100]`.
- **Boundary Examples**:
  - `"  Alice  "` $\rightarrow$ `Ok("Alice")`
  - `""`, `"   "` $\rightarrow$ `Err`
  - 100 `"a"` chars $\rightarrow$ `Ok`; 101 `"a"` chars $\rightarrow$ `Err`

### VO-08 — Email trims, lowercases, and enforces bounded RFC address syntax
- **Status**: `Draft`
- **Origin**: `human`
- **For all**: Input `&str` `raw` and normalized `norm = raw.trim().to_lowercase()`.
- **Then**: `Email::try_new(raw)` returns `Ok` containing `norm` iff `1 <= norm.len() <= 254` bytes, contains no whitespace/control chars, parses via `email_address` with required TLD (`>= 2` labels), unquoted local part, no display name, no domain literal, valid DNS/IDN labels (`1..=63`, no leading/trailing `-`), and valid TLD (`>= 2` ASCII letters or `xn--...`); otherwise returns `Err(EmailError::NotEmptyViolated | EmailError::PredicateViolated)` (`DomainError::InvalidEmail`).
- **Oracle**: Byte length, character hygiene, and domain/TLD rules on trimmed lowercased input.
- **Boundary Examples**:
  - `"  Alice@Example.COM  "` $\rightarrow$ `Ok("alice@example.com")`
  - `"alice@localhost"`, `"alice@[127.0.0.1]"`, `"Alice <alice@example.com>"`, `"\"quoted\"@example.com"`, `"alice@example.c"`, `"alice@example.123"`, `255 bytes` $\rightarrow$ `Err`

### VO-09 — Relationship trims whitespace and enforces 1..=50 Unicode characters
- **Status**: `Draft`
- **Origin**: `doc:documentation/architecture/database-schema.md`
- **For all**: Input `&str` `raw`.
- **Then**: `Relationship::try_new(raw)` returns `Ok` containing `raw.trim()` iff `1 <= trimmed.chars().count() <= 50`; otherwise returns `Err(RelationshipError::NotEmptyViolated)` or `Err(RelationshipError::LenCharMaxViolated)` (`DomainError::InvalidRelationship`).
- **Oracle**: Unicode char count of trimmed string in `[1, 50]`.
- **Boundary Examples**:
  - `"  Grandma  "` $\rightarrow$ `Ok("Grandma")`
  - `""`, `"   "` $\rightarrow$ `Err`
  - 50 `"a"` chars $\rightarrow$ `Ok`; 51 `"a"` chars $\rightarrow$ `Err`

### VO-10 — InviteTtl enforces 1..=2_592_000 whole seconds (max 30 days)
- **Status**: `Draft`
- **Origin**: `human`
- **For all**: `seconds: i64` or `duration: chrono::Duration`.
- **Then**:
  - `InviteTtl::try_new(seconds)` succeeds iff `1 <= seconds <= 2_592_000`; otherwise returns `Err(InviteTtlError::GreaterViolated | InviteTtlError::LessOrEqualViolated)` (`DomainError::InvalidInviteTtl`).
  - `InviteTtl::from_hours` / `from_days` reject arithmetic overflow with `DomainError::InvalidInviteTtl`.
  - `InviteTtl::try_from_duration` rejects `<= 0`, fractional sub-second durations (`subsec_nanos() != 0`), and `> 2_592_000` seconds.
- **Oracle**: Closed integer interval `[1, 2_592_000]` and zero sub-second remainder.
- **Boundary Examples**:
  - `1`, `from_days(30)` (`2_592_000`) $\rightarrow$ `Ok`
  - `0`, `-1`, `2_592_001`, `from_days(31)`, `1500ms` $\rightarrow$ `Err`

---

## Open Questions

*(None)*

---

## Findings

| # | Location | Code Says | Doc / Spec Says | Impact |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `backend/domain/src/value_objects/money.rs` | `Money::sub`, `Money::multiply`, checked arithmetic, and `try_from` deserialization enforce `>= 0` and overflow safety | `VO-01`–`VO-03`, `VO-06` (`Origin: human`): `Money` is strictly $\ge 0$; signed balances belong to `SignedMoney` / `AccountBalanceSnapshot` | Code aligned; spec `Draft` pending human approval |
