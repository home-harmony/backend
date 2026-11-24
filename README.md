# FamilyLedger Backend — Rust Workspace

Rust multi-crate workspace for the **HomeHarmony** (`FamilyLedger`) backend services.
Structured following Clean Architecture / Domain-Driven Design (DDD).

## Workspace Structure

```
backend/
├── Cargo.toml                  # Workspace definition, shared dependencies & lints
├── clippy.toml                 # Test exemptions for unwrap/expect
├── AGENTS.md                   # Backend Rust & Aurora DSQL entry point
├── .agents/skills.json         # Cross-repo skill registration
├── .ai/                        # Modular rules (backend-rust, aurora-dsql) & backend skills
├── specs/                      # Declarative behavior specifications (ADR-0019)
│   ├── domain/                 # Pure domain specs (VO, FM)
│   └── api/                    # API/handler specs (planned)
├── migrations/                 # Aurora DSQL SQL migration files (one DDL per file)
├── domain/                     # Pure domain logic — zero I/O dependencies
│   ├── src/
│   │   ├── entities/           # Aggregate roots (Family, FamilyMember, InviteToken)
│   │   ├── value_objects/      # Immutable VOs (Money, CurrencyCode, FamilyName, DisplayName, Email, Relationship, InviteTtl, Role, Permission, DomainAction, typed IDs)
│   │   ├── events/             # Domain events emitted by aggregates
│   │   ├── errors.rs           # Domain error types
│   │   └── lib.rs
│   └── tests/
│       ├── fm_family_rbac.rs   # Blind spec tests for FM-01..FM-22
│       ├── spec_traceability.rs # Spec <-> test governance gate (ADR-0019)
│       └── vo_value_objects.rs # Blind spec tests for VO-01..VO-10
├── infrastructure/             # Database repositories & external service adapters
│   ├── build.rs                # Triggers rebuild when migrations/*.sql change
│   ├── src/
│   │   └── db/
│   │       ├── migrations.rs   # Shared DSQL & PG16 migration engine + OCC retries
│   │       ├── pagination.rs   # Keyset cursor-based pagination utilities
│   │       └── pool.rs         # Aurora DSQL connection pool (IAM auth via connector)
│   └── tests/
│       ├── migration_invariants.rs # Offline DSQL migration invariant checks
│       └── migrations_pg.rs    # Docker PostgreSQL 16 integration test (#[ignore])
├── api/                        # Shared Axum middleware, auth extraction, RFC 9457 error mapping
│   └── src/
│       ├── auth.rs             # AuthClaims extractor (user_id, optional email, verified X-Family-Id)
│       ├── errors.rs           # DomainError & AuthError -> RFC 9457 application/problem+json
│       └── response.rs         # Standard response helpers
└── lambdas/
    └── migrate_runner/         # Lambda: applies SQL migrations at deploy time
        └── src/main.rs
```

## Tech Stack

- **Rust 1.100.0** with `edition = "2024"`
- **Axum 0.8** — async HTTP framework
- **SQLx 0.9** — compile-time verified SQL queries
- **Aurora DSQL** via `aurora-dsql-sqlx-connector` (IAM auth + OCC retry)
- **AWS Lambda** via `lambda_runtime` + `cargo-lambda`
- **`rust_decimal`** — exact decimal arithmetic for money (never `f64`)
- **`nutype` + `email_address`** — validated domain value objects at construction and deserialization
- **`uuid`** v4 for PKs/tokens, v7 for time-sortable append logs and transactions

## Developer Commands

```bash
# Run lints across all crates and tests (zero warnings enforced)
cargo clippy --workspace --all-targets -- -D warnings

# Run all workspace unit, integration, spec traceability, and doc tests
cargo test --workspace

# Run declarative spec <-> test traceability gate alone
cargo test -p domain --test spec_traceability

# Run pure domain tests (fast, no DB required)
cargo test -p domain

# Run offline infrastructure unit + migration invariant tests (no Docker required)
cargo test -p infrastructure

# Run Docker-backed PostgreSQL 16 integration tests (requires Docker active)
cargo test -p infrastructure -- --ignored

# Build Lambda binaries for ARM64
cargo lambda build --release --arm64

# Prepare SQLx offline query cache (for CI without a live DB)
cargo sqlx prepare --workspace -- --all-targets
```

## Non-Negotiable Rules

See [`AGENTS.md`](AGENTS.md) and [`documentation/`](../documentation/) for the full list. Key constraints:

1. **Money = `rust_decimal::Decimal`** — never `f64`/`f32`
2. **All PKs = UUID v4/v7** — no SERIAL, no auto-increment
3. **Keyset pagination only** — no OFFSET
4. **`family_id` via verified `X-Family-Id` membership** — never trust unverified request bodies or query params (ADR-0011)
5. **Soft-deletes** (`deleted_at`) — never hard DELETE business data
6. **One DDL per migration file** — Aurora DSQL constraint
7. **`BEGIN READ ONLY`** for all read-only Lambda handlers
