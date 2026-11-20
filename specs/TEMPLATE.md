# [PREFIX] — [Specification Title]

- **Prefix**: `XX`
- **Layer**: `domain` | `api`
- **Unit under test**: `crate::path::ToTypeOrHandler`
- **Test seam**: `Layer 1 (Pure Domain)` | `Layer 2a (Testcontainers PG)` | `Layer 2b (Axum oneshot + fakes)`
- **Format**: `T` (Case Table) | `U` (Use Case) | `P` (Property)
- **Test target**: `<crate>/tests/<prefix_lowercase>_<topic>.rs`

---

## Vocabulary

Map domain terms from [`ubiquitous-language.md`](../../documentation/domain/ubiquitous-language.md) to concrete public Rust API types and values.

| Term | Meaning / API Symbol | Concrete Representation |
| :--- | :--- | :--- |
| `ExampleTerm` | `domain::value_objects::Example` | Concrete representation / variant |

---

## Rules

<!-- KEEP ONE FORMAT SECTION BELOW AND DELETE THE OTHERS -->

### Option Format T: Case Table (State Machines / Decision Matrices)

| ID | Status | Origin | Given State | Action / Input | Expected Result / Next State |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `XX-01` | `Draft` | `observed` | Initial state | Public call & args | Expected outcome / `Err(DomainError::Variant)` |

---

### Option Format U: Use-Case Rules (API & Handler Flows)

### XX-01 — Brief Rule Description
- **Status**: `Draft`
- **Origin**: `human` | `adr:NNNN` | `doc:<path>` | `observed`
- **Given**: Preconditions, seeded repository/fake state, caller `AuthClaims` (`user_id`, `family_id`, `role`).
- **When**: Single API / service call (e.g. `POST /families/{id}/invites` via `tower::ServiceExt::oneshot()`).
- **Then**: Observable HTTP status, response body / error code (`INSUFFICIENT_ROLE`), and persisted repository/event state.
- **Notes**: Context or ADR links.

---

### Option Format P: Property Invariants (Value Objects & Calculators)

### XX-01 — Invariant Name
- **Status**: `Draft`
- **Origin**: `human` | `adr:NNNN` | `doc:<path>` | `observed`
- **For all**: Input domain (e.g., any non-negative `Decimal` and valid 3-char uppercase ASCII `CurrencyCode`).
- **Then**: `Type::try_new(input)` or method call satisfies observable invariant.
- **Oracle**: Independent validation rule or mathematical law (never copying production logic).
- **Boundary Examples**: Concrete valid and invalid edge cases (`""`, `200 chars`, `201 chars`, `-0.01`, `0.00`).

---

## Open Questions

Ambiguities requiring human decision. Rules mentioned here are blocked by `spec_traceability` from being marked `Approved`.

1. **Q1 (`XX-01`)**: Description of open ambiguity.

---

## Findings

Discrepancies noticed between code, docstrings, ADRs, and existing tests.

| # | Location | Code Says | Doc / ADR Says | Impact |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `backend/...` | ... | ... | ... |
