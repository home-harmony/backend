# Worked Behavior Specification Examples — FamilyLedger

Illustrative examples demonstrating Formats **P**, **T**, and **U** using canonical terms from [`ubiquitous-language.md`](../../documentation/domain/ubiquitous-language.md). *(Examples use an illustrative `EX` prefix so they never collide with live rules in `specs/domain/` or `specs/api/`.)*

---

## 1. Format P Example — `Money` Currency Safety (`EX-01`)

### EX-01 — Currency mismatch rejects addition
- **Status**: `Draft`
- **Origin**: `adr:0005`
- **For all**: Two `Money` values `a(amount_a, cur_a)` and `b(amount_b, cur_b)` where `cur_a != cur_b`.
- **Then**: `a.add(&b)` returns `Err(DomainError::CurrencyMismatch { expected: cur_a, actual: cur_b })`.
- **Oracle**: Currency equality is symmetric and strict (`cur_a.as_str() != cur_b.as_str()`).
- **Boundary Examples**:
  - `Money(10.00 USD).add(&Money(5.00 EUR))` $\rightarrow$ `Err(DomainError::CurrencyMismatch { expected: USD, actual: EUR })`.
  - `Money(0.00 USD).add(&Money(0.00 EUR))` $\rightarrow$ still `Err(DomainError::CurrencyMismatch)` (zero does not bypass currency check).

---

## 2. Format T Example — Role Debt Access Matrix (`EX-02` – `EX-06`)

| ID | Status | Origin | Given Role & Custom Permissions | Action (`assert_debt_access`) | Expected Outcome |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `EX-02` | `Draft` | `doc:documentation/domain/bounded-contexts.md` | `Role::Owner`, `custom = []` | `assert_debt_access(&custom)` | `Ok(())` |
| `EX-03` | `Draft` | `doc:documentation/domain/bounded-contexts.md` | `Role::Member`, `custom = []` | `assert_debt_access(&custom)` | `Ok(())` |
| `EX-04` | `Draft` | `human` | `Role::Other`, `custom = [Permission::ViewLoans]` | `assert_debt_access(&custom)` | `Ok(())` |
| `EX-05` | `Draft` | `human` | `Role::Other`, `custom = []` | `assert_debt_access(&custom)` | `Err(DomainError::InsufficientRole { role: Role::Other })` |
| `EX-06` | `Draft` | `doc:documentation/domain/bounded-contexts.md` | `Role::Child`, `custom = [Permission::ViewLoans]` | `assert_debt_access(&custom)` | `Err(DomainError::InsufficientRole { role: Role::Child })` |

---

## 3. Format U Example — Owner Budget Approval (`EX-07` — Illustrative)

### EX-07 — Member cannot approve a `BudgetDraft`
- **Status**: `Draft`
- **Origin**: `adr:0013`
- **Given**: A `MonthlyBudget` in `Draft` status belonging to `family_A`, and a caller authenticated with `X-Family-Id: family_A` holding `Role::Member`.
- **When**: `POST /budgets/{month}/approve` is invoked via Axum `oneshot`.
- **Then**: Returns `403 Forbidden` with Problem Details code `INSUFFICIENT_ROLE`; the budget remains `Draft` in repository state and emits zero `BudgetApproved` events in `SpyPublisher`.
