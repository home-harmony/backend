# VO — Core Domain Value Objects

- **Prefix**: `VO`
- **Layer**: `domain`
- **Unit under test**: `domain::value_objects::{Money, CurrencyCode, FamilyName, MemberDisplayName, Email, Relationship, InviteTtl}`
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
| `MemberDisplayName` | `domain::value_objects::MemberDisplayName` | Validated non-empty trimmed string ($\le 100$ Unicode chars) |
| `Email` | `domain::value_objects::Email` | Trimmed, lowercased email address (`<= 254` bytes, unquoted local part, required TLD, no display text or domain literal) |
| `Relationship` | `domain::value_objects::Relationship` | Validated non-empty trimmed relationship label ($\le 50$ Unicode chars) |
| `InviteTtl` | `domain::value_objects::InviteTtl` | Bounded positive expiration duration in seconds (`1..=2_592_000`, max 30 days) |
| `InviteTokenHash` | `domain::value_objects::InviteTokenHash` | Validated 64-char lowercase ASCII hex SHA-256 digest of an `InviteTokenId` |
| `SignedMoney` | `domain::value_objects::SignedMoney` | Signed float-free amount (`rust_decimal::Decimal`, `< 0 \| 0 \| > 0`) + `CurrencyCode` for account balances, snapshots, and `FXSpread` |
| `PositiveMoney` | `domain::value_objects::PositiveMoney` | Strictly positive float-free amount (`rust_decimal::Decimal > 0`) + `CurrencyCode` for ledger transaction legs |
| `AccountName` | `domain::value_objects::AccountName` | Trimmed non-empty string (`1..=200` Unicode chars, no control/BiDi chars) |
| `BankName` | `domain::value_objects::BankName` | Trimmed non-empty string (`1..=100` Unicode chars, no control/BiDi chars) |
| `LastFour` | `domain::value_objects::LastFour` | Trimmed 4-character ASCII digit string (`'0'..='9'`) |
| `HexColor` | `domain::value_objects::HexColor` | Trimmed 7-character `#RRGGBB` hex color normalized to uppercase |
| `YearMonth` | `domain::value_objects::YearMonth` | Validated `'YYYY-MM'` calendar month (`year` in `1970..=9999`, `month` in `01..=12`) |
| `CategoryName` | `domain::value_objects::CategoryName` | Trimmed non-empty string (`1..=100` Unicode chars, no control/BiDi chars) |
| `CategoryIcon` | `domain::value_objects::CategoryIcon` | Trimmed non-empty icon identifier (`1..=50` Unicode chars, no control/BiDi chars) |
| `Tag` | `domain::value_objects::Tag` | Trimmed non-empty tag string (`1..=40` Unicode chars, no control/BiDi chars) |
| `TransactionDescription` | `domain::value_objects::TransactionDescription` | Trimmed non-empty note (`1..=500` Unicode chars, no control/BiDi chars) |
| `DomainError` | `domain::errors::DomainError` | Enum variants (`CurrencyMismatch`, `NegativeAmount`, `NonPositiveAmount`, `ArithmeticOverflow`, `InvalidCurrencyCode`, `InvalidFamilyName`, `InvalidDisplayName`, `InvalidEmail`, `InvalidRelationship`, `InvalidInviteTtl`, `InvalidAccountName`, `InvalidBankName`, `InvalidLastFour`, `InvalidHexColor`, `InvalidYearMonth`, `InvalidCategoryName`, `InvalidCategoryIcon`, `InvalidTag`, `InvalidDescription`) |

---

## Rules

### VO-01 — Money construction rejects negative amounts and accepts zero or positive decimals
- **Status**: `Approved`
- **Origin**: `adr:0005`
- **For all**: `amount: Decimal` and valid `currency: CurrencyCode`.
- **Then**: `Money::new(amount, currency)` returns `Ok(m)` with `m.amount() == amount` when `amount >= 0`, and `Err(DomainError::NegativeAmount { amount })` when `amount < 0`.
- **Oracle**: Sign comparison against `Decimal::ZERO`.
- **Boundary Examples**:
  - `0.00 USD` $\rightarrow$ `Ok` (`is_zero() == true`)
  - `0.01 USD` $\rightarrow$ `Ok`
  - `-0.01 USD` $\rightarrow$ `Err(DomainError::NegativeAmount { .. })`

### VO-02 — Money addition requires matching CurrencyCode and detects overflow
- **Status**: `Approved`
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
- **Status**: `Approved`
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
- **Status**: `Approved`
- **Origin**: `human`
- **For all**: Input `&str` `raw`.
- **Then**: `CurrencyCode::try_new(raw)` returns `Ok` containing `raw.trim()` iff `raw.trim()` has length `3` and all bytes are ASCII `'A'..='Z'`; otherwise returns `Err(CurrencyCodeError::PredicateViolated)` (which converts into `DomainError::InvalidCurrencyCode { .. }`).
- **Oracle**: ASCII uppercase length-3 predicate on trimmed input.
- **Boundary Examples**:
  - `"USD"` $\rightarrow$ `Ok("USD")`, `"  EUR  "` $\rightarrow$ `Ok("EUR")`
  - `"usd"`, `"US"`, `"USDD"`, `""`, `"U$D"` $\rightarrow$ `Err`

### VO-05 — FamilyName trims whitespace and enforces 1..=200 Unicode characters
- **Status**: `Approved`
- **Origin**: `doc:documentation/design-review-2026-08-28.md`
- **For all**: Input `&str` `raw`.
- **Then**: `FamilyName::try_new(raw)` returns `Ok` containing `raw.trim()` iff `1 <= trimmed.chars().count() <= 200`; otherwise returns `Err(FamilyNameError::NotEmptyViolated)` or `Err(FamilyNameError::LenCharMaxViolated)` (which convert into `DomainError::InvalidFamilyName { .. }`).
- **Oracle**: Unicode char count of trimmed string in `[1, 200]`.
- **Boundary Examples**:
  - `"  Smith Family  "` $\rightarrow$ `Ok("Smith Family")`
  - `""`, `"   "` $\rightarrow$ `Err(FamilyNameError::NotEmptyViolated)`
  - 200 `"a"` chars $\rightarrow$ `Ok`; 201 `"a"` chars $\rightarrow$ `Err(FamilyNameError::LenCharMaxViolated)`

### VO-06 — Money multiplication and deserialization preserve non-negative and overflow invariants
- **Status**: `Approved`
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

### VO-07 — MemberDisplayName trims whitespace and enforces 1..=100 Unicode characters
- **Status**: `Approved`
- **Origin**: `doc:documentation/architecture/database-schema.md`
- **For all**: Input `&str` `raw`.
- **Then**: `MemberDisplayName::try_new(raw)` returns `Ok` containing `raw.trim()` iff `1 <= trimmed.chars().count() <= 100`; otherwise returns `Err(MemberDisplayNameError::NotEmptyViolated)` or `Err(MemberDisplayNameError::LenCharMaxViolated)` (`DomainError::InvalidDisplayName`).
- **Oracle**: Unicode char count of trimmed string in `[1, 100]`.
- **Boundary Examples**:
  - `"  Alice  "` $\rightarrow$ `Ok("Alice")`
  - `""`, `"   "` $\rightarrow$ `Err`
  - 100 `"a"` chars $\rightarrow$ `Ok`; 101 `"a"` chars $\rightarrow$ `Err`

### VO-08 — Email trims, lowercases, and enforces bounded RFC address syntax
- **Status**: `Approved`
- **Origin**: `human`
- **For all**: Input `&str` `raw` and normalized `norm = raw.trim().to_lowercase()`.
- **Then**: `Email::try_new(raw)` returns `Ok` containing `norm` iff `1 <= norm.len() <= 254` bytes, contains no whitespace/control chars, satisfies RFC email syntax with required TLD (`>= 2` labels), unquoted local part, no display name, no domain literal, valid DNS/IDN labels (`1..=63`, no leading/trailing `-`), and valid TLD (`>= 2` ASCII letters or `xn--...`); otherwise returns `Err(EmailError::NotEmptyViolated | EmailError::PredicateViolated)` (`DomainError::InvalidEmail`).
- **Oracle**: Byte length, character hygiene, and domain/TLD rules on trimmed lowercased input.
- **Boundary Examples**:
  - `"  Alice@Example.COM  "` $\rightarrow$ `Ok("alice@example.com")`
  - `"alice@localhost"`, `"alice@[127.0.0.1]"`, `"Alice <alice@example.com>"`, `"\"quoted\"@example.com"`, `"alice@example.c"`, `"alice@example.123"`, `255 bytes` $\rightarrow$ `Err`

### VO-09 — Relationship trims whitespace and enforces 1..=50 Unicode characters
- **Status**: `Approved`
- **Origin**: `doc:documentation/architecture/database-schema.md`
- **For all**: Input `&str` `raw`.
- **Then**: `Relationship::try_new(raw)` returns `Ok` containing `raw.trim()` iff `1 <= trimmed.chars().count() <= 50`; otherwise returns `Err(RelationshipError::NotEmptyViolated)` or `Err(RelationshipError::LenCharMaxViolated)` (`DomainError::InvalidRelationship`).
- **Oracle**: Unicode char count of trimmed string in `[1, 50]`.
- **Boundary Examples**:
  - `"  Grandma  "` $\rightarrow$ `Ok("Grandma")`
  - `""`, `"   "` $\rightarrow$ `Err`
  - 50 `"a"` chars $\rightarrow$ `Ok`; 51 `"a"` chars $\rightarrow$ `Err`

### VO-10 — InviteTtl enforces 1..=2_592_000 whole seconds (max 30 days)
- **Status**: `Approved`
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

### VO-11 — Display text value objects reject control characters and Unicode BiDi overrides
- **Status**: `Approved`
- **Origin**: `human`
- **For all**: Input `&str` `raw` passed to `FamilyName::try_new`, `MemberDisplayName::try_new`, or `Relationship::try_new`.
- **Then**: Construction and JSON deserialization reject strings whose trimmed content contains any `char::is_control()` character or Unicode bidirectional override/isolate formatting character (`U+202A..=U+202E`, `U+2066..=U+2069`, `U+200E`, `U+200F`, `U+061C`) with `PredicateViolated` (converting to `DomainError::InvalidFamilyName` / `InvalidDisplayName` / `InvalidRelationship`). Empty and over-length checks still take precedence (`NotEmptyViolated`, `LenCharMaxViolated`).
- **Oracle**: Character-level scan after trim for control and BiDi formatting code points.
- **Boundary Examples**:
  - `"Alice\0"`, `"Alice\nBob"`, `"Alice\x1b[31m"`, `"Alice\u{202E}boB"` $\rightarrow$ `Err`
  - `"  Família da Silva  "` $\rightarrow$ `Ok`

### VO-12 — InviteTokenHash computes deterministic 64-char lowercase hex SHA-256 digest
- **Status**: `Approved`
- **Origin**: `human`
- **For all**: `token: InviteTokenId` or raw `&str` hex digest.
- **Then**:
  - `InviteTokenHash::from_token_id(token)` returns a deterministic 64-character lowercase ASCII hex SHA-256 digest of `token.to_string().as_bytes()`.
  - `InviteTokenHash::try_new(raw)` trims whitespace and succeeds iff the trimmed string has length `64` and all characters are lowercase ASCII hex (`'0'..='9' | 'a'..='f'`); uppercase hex, non-hex, or wrong-length strings fail at construction and deserialization.
  - `hash.matches_token(candidate)` returns `true` iff `InviteTokenHash::from_token_id(candidate) == *hash`.
- **Oracle**: SHA-256 over canonical UUID hyphenated lowercase string + lowercase hex format predicate.
- **Boundary Examples**:
  - `InviteTokenHash::from_token_id(t).as_str().len()` == `64`
  - 64 `'A'` chars (uppercase), 63 `'a'` chars, 64 `'g'` chars $\rightarrow$ `Err`

### VO-13 — SignedMoney supports negative, zero, and positive decimals with currency-safe checked arithmetic
- **Status**: `Approved`
- **Origin**: `adr:0006`
- **For all**: `SignedMoney` values `a(amt_a, cur_a)` and `b(amt_b, cur_b)` across negative, zero, and positive `Decimal` values.
- **Then**:
  - `SignedMoney::new(amount, currency)` succeeds for `< 0`, `0`, and `> 0`.
  - `a.add(&b)` and `a.sub(&b)` return `Err(DomainError::CurrencyMismatch { .. })` when `cur_a != cur_b`, `Err(DomainError::ArithmeticOverflow { .. })` on `Decimal` overflow/underflow, and exact signed results otherwise.
  - `a.negate()` returns `Err(DomainError::ArithmeticOverflow { .. })` only if negation overflows, and `SignedMoney(-amt_a, cur_a)` otherwise.
- **Oracle**: Checked exact signed `Decimal` arithmetic gated on matching `CurrencyCode`.
- **Boundary Examples**:
  - `-120.50 USD + 20.50 USD` $\rightarrow$ `Ok(-100.00 USD)`
  - `10.00 USD - 25.00 USD` $\rightarrow$ `Ok(-15.00 USD)`
  - `-10.00 USD + 10.00 EUR` $\rightarrow$ `Err(DomainError::CurrencyMismatch { .. })`

### VO-14 — PositiveMoney strictly requires amounts greater than zero
- **Status**: `Approved`
- **Origin**: `human`
- **For all**: `amount: Decimal` and valid `currency: CurrencyCode` (or JSON `{"amount": ..., "currency": ...}`).
- **Then**: `PositiveMoney::new(amount, currency)` (and serde deserialization) returns `Ok(pm)` when `amount > Decimal::ZERO`, and `Err(DomainError::NonPositiveAmount { amount })` when `amount <= Decimal::ZERO`. Conversion `Into<Money>` / `as_money()` preserves exact `amount` and `currency`.
- **Oracle**: Strict comparison `amount > Decimal::ZERO`.
- **Boundary Examples**:
  - `0.0001 USD` $\rightarrow$ `Ok`
  - `0.00 USD`, `-0.01 USD` $\rightarrow$ `Err(DomainError::NonPositiveAmount { .. })`

### VO-15 — AccountName and BankName enforce trimmed character bounds and reject control/BiDi characters
- **Status**: `Approved`
- **Origin**: `doc:documentation/architecture/database-schema.md`
- **For all**: Input `&str` `raw`.
- **Then**:
  - `AccountName::try_new(raw)` trims whitespace and succeeds iff `1 <= chars <= 200` and contains no control or Unicode BiDi override/isolate code points (`Err` -> `DomainError::InvalidAccountName`).
  - `BankName::try_new(raw)` trims whitespace and succeeds iff `1 <= chars <= 100` and contains no control or Unicode BiDi override/isolate code points (`Err` -> `DomainError::InvalidBankName`).
- **Oracle**: Trimmed Unicode char count within `[1, max]` + control/BiDi character exclusion.
- **Boundary Examples**:
  - `AccountName`: 1 char, 200 chars $\rightarrow$ `Ok`; `""`, `"   "`, 201 chars, `"Card\0"` $\rightarrow$ `Err`
  - `BankName`: 1 char, 100 chars $\rightarrow$ `Ok`; `""`, 101 chars, `"Bank\u{202E}"` $\rightarrow$ `Err`

### VO-16 — LastFour enforces exactly 4 ASCII digits and HexColor enforces normalized #RRGGBB
- **Status**: `Approved`
- **Origin**: `doc:documentation/architecture/database-schema.md`
- **For all**: Input `&str` `raw`.
- **Then**:
  - `LastFour::try_new(raw)` trims whitespace and succeeds iff length is `4` and every byte is `'0'..='9'` (`Err` -> `DomainError::InvalidLastFour`).
  - `HexColor::try_new(raw)` trims whitespace, normalizes ASCII hex letters to uppercase, and succeeds iff formatted as `#` followed by `6` ASCII hex digits (`Err` -> `DomainError::InvalidHexColor`).
- **Oracle**: Exact regex-equivalent byte predicates (`^\d{4}$` and `^#[0-9A-Fa-f]{6}$`).
- **Boundary Examples**:
  - `LastFour`: `"0042"` $\rightarrow$ `Ok`; `"42"`, `"12345"`, `"12a4"` $\rightarrow$ `Err`
  - `HexColor`: `"#1a2b3c"` $\rightarrow$ `Ok("#1A2B3C")`; `"1A2B3C"`, `"#FFF"`, `"#12345G"` $\rightarrow$ `Err`

### VO-17 — YearMonth validates YYYY-MM format and computes exact UTC end-of-month cutoff
- **Status**: `Approved`
- **Origin**: `adr:0006`
- **For all**: Input `&str` `raw` or `(year: i32, month: u32)`.
- **Then**:
  - `YearMonth::try_new(raw)` trims whitespace and succeeds iff formatted as `YYYY-MM` with `1970 <= year <= 9999` and `1 <= month <= 12` (`Err` -> `DomainError::InvalidYearMonth`).
  - `ym.end_of_month_utc()` returns the last nanosecond/microsecond instant of that calendar month in UTC (accounting for 28/29-day February leap years and 30/31-day months), such that any timestamp strictly in the next month is `> ym.end_of_month_utc()`.
- **Oracle**: Gregorian calendar month-length rules (`chrono::NaiveDate`).
- **Boundary Examples**:
  - `"2024-02"` (leap) $\rightarrow$ `2024-02-29T23:59:59.999999Z`; `"2026-02"` $\rightarrow$ `2026-02-28T23:59:59.999999Z`
  - `"2026-00"`, `"2026-13"`, `"26-03"`, `"2026/03"` $\rightarrow$ `Err`

### VO-18 — CategoryName, CategoryIcon, Tag, and TransactionDescription enforce trimmed bounds and hygiene
- **Status**: `Approved`
- **Origin**: `doc:documentation/architecture/database-schema.md`
- **For all**: Input `&str` `raw`.
- **Then**: Trims whitespace, rejects control and Unicode BiDi override/isolate characters, and enforces exact character bounds:
  - `CategoryName`: `1..=100` (`DomainError::InvalidCategoryName`)
  - `CategoryIcon`: `1..=50` (`DomainError::InvalidCategoryIcon`)
  - `Tag`: `1..=40` (`DomainError::InvalidTag`); tag lists enforce `<= 20` deduplicated items (`DomainError::TooManyTags`)
  - `TransactionDescription`: `1..=500` (`DomainError::InvalidDescription`; all `char::is_control()` characters rejected).
- **Oracle**: Trimmed Unicode char count in `[1, max]` + `is_control()` / BiDi rejection + max 20 deduplicated tags.
- **Boundary Examples**:
  - `CategoryName`: 100 chars $\rightarrow$ `Ok`, 101 $\rightarrow$ `Err`; `CategoryIcon`: 50 $\rightarrow$ `Ok`, 51 $\rightarrow$ `Err`
  - `Tag`: 40 chars $\rightarrow$ `Ok`, 41 $\rightarrow$ `Err`; 21 distinct tags $\rightarrow$ `Err`
  - `TransactionDescription`: 500 chars $\rightarrow$ `Ok`, 501 or `"note\0"` $\rightarrow$ `Err`

---

## Open Questions

*(None)*

---

## Findings

| # | Location | Code Says | Doc / Spec Says | Impact |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `backend/domain/src/value_objects/money.rs` | `Money::sub`, `Money::multiply`, checked arithmetic, and `try_from` deserialization enforce `>= 0` and overflow safety | `VO-01`–`VO-03`, `VO-06` (`Origin: human`): `Money` is strictly $\ge 0$; signed balances belong to `SignedMoney` / `AccountBalanceSnapshot` | Spec `Approved`; covered by `domain/tests/vo_value_objects.rs` |
