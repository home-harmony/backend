---
name: rust-patterns
description: >-
  Use when designing, implementing, or refactoring Rust code in backend/.
  Covers Deep Modules (Deletion Test), SLAP/SRP structure, Newspaper ordering,
  RPITIT static dispatch ports, nutype value objects, Decimal math, ACL DTOs,
  and thiserror-to-API mapping.
---

# Rust Clean Code & Architectural Patterns

Procedural design guide for crates in `backend/` (`domain`, `infrastructure`, `api`, `lambdas/*`). Applies the invariants in [`.ai/rules/backend-rust.md`](../../rules/backend-rust.md) without duplicating them.

---

## 1. Design Checks Before Writing Code

1. **Deletion Test (Deep Modules)**: Before introducing a struct, wrapper, or trait, verify that removing it would scatter real complexity across callers rather than eliminating a hollow pass-through.
2. **Boundary Placement**:
   - Keep entities, VOs, events, `DomainError`, and repository/provider ports in `domain/` (strictly zero I/O).
   - Keep `serde` wire DTOs (`api/`) and `sqlx::FromRow` rows (`infrastructure/`) outside `domain/` when shapes differ; map via `TryFrom` or dedicated ACL mappers, and reconstitute private-field aggregates via explicit `reconstitute(...)` parameter structs.
3. **RPITIT Static-Dispatch Port Pattern**:
   ```rust
   pub trait AccountRepository: Send + Sync {
       fn find_by_id(&self, family_id: FamilyId, id: AccountId)
           -> impl Future<Output = Result<Option<PaymentAccount>, DomainError>> + Send;
   }
   ```
   Inject via generic type parameters (`struct Service<R> { repo: R }`) for zero-cost static dispatch with adapters and test fakes. Reserve `Arc<dyn Trait>` for synchronous traits or open registries.

---

## 2. Modeling Idioms

- **Uniform `nutype` Value Objects**: Validate invariants at construction and deserialization using `try_new` (`CurrencyCode::try_new`, `FamilyName::try_new`, `DisplayName::try_new`, `Email::try_new`, `Relationship::try_new`, `InviteTtl::try_new`), expose `.as_str()` on string VOs, and map `*Error` into `DomainError` via `From`.
- **Enum State Machines & Deterministic Clocks**: Model aggregate lifecycles (`MonthlyBudget` `Draft`/`Active`, `Transaction` kinds) via enums; expose `*_at(..., now: DateTime<Utc>)` core methods with thin `Utc::now()` wrappers so Layer 1 tests stay deterministic.
- **Checked Financial Math**: Enforce `Money >= 0` on construction, deserialization (`#[serde(try_from = "...")]`), subtraction, and multiplication; use `checked_add` / `checked_sub` / `checked_mul` returning `DomainError::ArithmeticOverflow`. Use `SignedMoney` *(planned Sprint 3)* when negative balances are domain-valid (`AccountBalanceSnapshot`).
- **Central Exhaustive Error Mapping**: Map every `DomainError` variant exhaustively (no `_ =>` wildcard) in `api/src/errors.rs` to RFC 9457 `application/problem+json` with a distinct machine-readable `code`.

---

## 3. Implementation Sequence

1. Confirm test seams (`rust-testing`) and inspect relevant `backend/specs/` rules.
2. Structure each file per [`backend-rust.md`](../../rules/backend-rust.md) (SLAP orchestrators vs. leaf workers, `10–30` lines / max `40`, max nesting `2`, Newspaper ordering).
3. Implement vertically and verify continuously:
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
4. Execute `code-review` before finishing.
