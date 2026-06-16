# FamilyLedger Backend — Rust Workspace

Rust multi-crate workspace for the FamilyLedger backend services.
Structured following Clean Architecture / Domain-Driven Design (DDD).

## Workspace Structure

```
backend/
├── Cargo.toml                  # Workspace definition & shared dependency versions
├── migrations/                 # Aurora DSQL SQL migration files (one DDL per file)
├── domain/                     # Pure domain logic — zero I/O dependencies
│   └── src/
│       ├── entities/           # Aggregate roots (Family, FamilyMember, ...)
│       ├── value_objects/      # Immutable domain values (Money, CurrencyCode, Role)
│       ├── events/             # Domain events emitted by aggregates
│       └── errors.rs           # Domain error types
├── infrastructure/             # Database repositories & external service adapters
│   └── src/
│       └── db/
│           ├── pool.rs         # Aurora DSQL connection pool (IAM auth via connector)
│           └── pagination.rs   # Keyset cursor-based pagination utilities
├── api/                        # Shared Axum middleware, JWT extraction, error mapping
│   └── src/
│       ├── auth.rs             # AuthClaims extractor (family_id always from JWT)
│       ├── errors.rs           # DomainError → HTTP response mapping
│       └── response.rs         # Standard response helpers
└── lambdas/
    └── migrate_runner/         # Lambda: applies SQL migrations at deploy time
        └── src/main.rs
```

## Tech Stack

- **Rust 1.96.0** with `edition = "2024"`
- **Axum 0.8** — async HTTP framework
- **SQLx 0.9** — compile-time verified SQL queries
- **Aurora DSQL** via `aurora-dsql-sqlx-connector` (IAM auth + OCC retry)
- **AWS Lambda** via `lambda_runtime` + `cargo-lambda`
- **`rust_decimal`** — exact decimal arithmetic for money (never `f64`)
- **`uuid`** v4 for PKs, v7 for time-sortable transaction IDs

## Developer Commands

```powershell
# Run pure domain unit tests (fast, no DB required)
cargo test -p domain

# Run infrastructure integration tests (requires Docker for PostgreSQL)
cargo test -p infrastructure

# Build Lambda binaries for ARM64
cargo lambda build --release --arm64

# Prepare SQLx offline query cache (for CI without a live DB)
cargo sqlx prepare -- --all-targets
```

## Non-Negotiable Rules

See [GEMINI.md](../documentation/) for the full list. Key constraints:

1. **Money = `rust_decimal::Decimal`** — never `f64`/`f32`
2. **All PKs = UUID v4/v7** — no SERIAL, no auto-increment
3. **Keyset pagination only** — no OFFSET
4. **`family_id` from JWT only** — never trust client input
5. **Soft-deletes** (`deleted_at`) — never hard DELETE business data
6. **One DDL per migration file** — Aurora DSQL constraint
7. **`BEGIN READ ONLY`** for all read-only Lambda handlers
