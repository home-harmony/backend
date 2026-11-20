# Worked Behavior Specification Examples — FamilyLedger

Reference examples illustrating Formats **T**, **P**, and **U** using canonical terms from [`ubiquitous-language.md`](../../documentation/domain/ubiquitous-language.md).

---

## 1. Format P Example — `Money` Currency Safety (`VO-01`)

### VO-01 — Currency mismatch rejects addition
- **Status**: `Draft`
- **Origin**: `adr:0005`
- **For all**: Two `Money` values `a(amount_a, cur_a)` and `b(amount_b, cur_b)` where `cur_a != cur_b`.
- **Then**: `a.add(&b)` returns `Err(DomainError::CurrencyMismatch { expected: cur_a, actual: cur_b })`.
- **Oracle**: Currency equality is symmetric and strict (`cur_a.as_str() != cur_b.as_str()`).
- **Boundary Examples**:
  - `Money(10.00 USD).add(&Money(5.00 EUR))` $\rightarrow$ `Err(DomainError::CurrencyMismatch { expected: USD, actual: EUR })`.
  - `Money(0.00 USD).add(&Money(0.00 EUR))` $\rightarrow$ still `Err(DomainError::CurrencyMismatch)` (zero does not bypass currency check).

---

## 2. Format T Example — Role Debt Access Matrix (`FM-01` – `FM-04`)

| ID | Status | Origin | Role | Action (`can_access_debt_features` / `assert_debt_access`) | Expected Outcome |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `FM-01` | `Draft` | `doc:documentation/domain/bounded-contexts.md` | `Role::Owner` | `assert_debt_access()` | `Ok(())` (`true`) |
| `FM-02` | `Draft` | `doc:documentation/domain/bounded-contexts.md` | `Role::Member` | `assert_debt_access()` | `Ok(())` (`true`) |
| `FM-03` | `Draft` | `doc:documentation/domain/bounded-contexts.md` | `Role::Other` | `assert_debt_access()` | `Ok(())` (`true`) |
| `FM-04` | `Draft` | `doc:documentation/domain/bounded-contexts.md` | `Role::Child` | `assert_debt_access()` | `Err(DomainError::InsufficientRole { role: Role::Child })` (`false`) |

---

## 3. Format U Example — Owner Budget Approval (`BG-01` — Illustrative)

### BG-01 — Member cannot approve a `BudgetDraft`
- **Status**: `Draft`
- **Origin**: `adr:0013`
- **Given**: A `MonthlyBudget` in `Draft` status belonging to `family_A`, and a caller authenticated with `X-Family-Id: family_A` holding `Role::Member`.
- **When**: `POST /budgets/{month}/approve` is invoked via Axum `oneshot`.
- **Then**: Returns `403 Forbidden` with error code `INSUFFICIENT_ROLE`; the budget remains `Draft` in repository state and emits zero `BudgetApproved` events in `SpyPublisher`.
