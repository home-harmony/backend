# AU — Multi-Family Authentication & `X-Family-Id` Isolation

- **Prefix**: `AU`
- **Layer**: `api`
- **Unit under test**: `api::auth::{UserIdentity, AuthClaims, family_auth_middleware, LocalJwtVerifier}` and `family_service` HTTP router
- **Test seam**: `Layer 2b (Axum oneshot + fakes)`
- **Format**: `U` (Use Case)
- **Test target**: `api/tests/au_multi_family_auth.rs`

---

## Vocabulary

| Term | Meaning / API Symbol | Concrete Representation |
| :--- | :--- | :--- |
| `UserIdentity` | `api::auth::UserIdentity` | Verified caller identity extracted from API Gateway's Cognito JWT authorizer context (`user_id: UserId` from `sub`, optional `email: Option<Email>`). |
| `AuthClaims` | `api::auth::AuthClaims` | Verified family-scoped context (`user_id`, `family_id`, `member_id`, `role`, `permissions`, `email`) resolved from `UserIdentity` + active `family_members` row for `X-Family-Id`. |
| `MembershipReader` | `domain::ports::MembershipReader` | RPITIT port queried by auth middleware to resolve `(member_id, role, permissions)` for active (`deleted_at IS NULL`) `(family_id, user_id)` membership. |
| `LocalJwtVerifier` | `api::auth::LocalJwtVerifier` | Local-development JWT verification adapter (signature, issuer, audience, expiry) producing `UserIdentity`; never used behind API Gateway. |
| `X-Family-Id` | HTTP request header `X-Family-Id: <UUID>` | Client-supplied workspace selector validated as UUID and verified against `family_members` per request (`ADR-0011`). |
| `ApiError` | `api::errors::ApiError` | RFC 9457 `application/problem+json` response carrying HTTP `status` and machine-readable `code` (`MISSING_CLAIMS`, `MISSING_FAMILY_ID`, `INVALID_FAMILY_ID`, `FAMILY_ID_MISMATCH`, `USER_NOT_IN_FAMILY`, `INSUFFICIENT_ROLE`). |

---

## Rules

### AU-01 — Missing verified `UserIdentity` returns 401 `MISSING_CLAIMS`
- **Status**: `Approved`
- **Origin**: `adr:0011`
- **Given**: An HTTP request to any protected route (`/profile/families`, `POST /families`, or any `/families/{id}*` endpoint) without verified `UserIdentity` claims in request extensions.
- **When**: The request is dispatched through the Axum router via `tower::ServiceExt::oneshot()`.
- **Then**: Responds with `401 Unauthorized`, `Content-Type: application/problem+json`, and `code == "MISSING_CLAIMS"`.
- **Notes**: API Gateway HTTP API `CognitoJwtAuthorizer` populates JWT claims before Lambda execution; missing claims fail closed.

### AU-02 — Identity-only routes succeed with `UserIdentity` and no `X-Family-Id`
- **Status**: `Approved`
- **Origin**: `adr:0011`
- **Given**: Valid `UserIdentity` (`sub` UUID, optional `email`) and no `X-Family-Id` header on the request.
- **When**: Caller invokes `GET /profile/families`, `POST /families`, or `POST /families/{id}/members` (with a valid unused invite token).
- **Then**: Request bypasses `X-Family-Id` membership middleware and succeeds (`200 OK` or `201 Created`) without failing on missing `X-Family-Id`.
- **Notes**: Users with zero families or accepting a first invite do not have an active `X-Family-Id` membership yet.

### AU-03 — Family-scoped route rejects missing or malformed `X-Family-Id`
- **Status**: `Approved`
- **Origin**: `adr:0011`
- **Given**: Valid `UserIdentity` on a family-scoped route (`GET /families/{id}`, `PUT /families/{id}`, `POST /families/{id}/invites`, `GET /families/{id}/members`, `PATCH /families/{id}/members/{mid}/role`, `DELETE /families/{id}/members/{mid}`).
- **When**: `X-Family-Id` header is absent/empty, OR `X-Family-Id` is not a valid UUID (e.g., `"not-a-uuid"`).
- **Then**: Responds with `400 Bad Request`, `Content-Type: application/problem+json`, and `code == "MISSING_FAMILY_ID"` (when absent/empty) or `code == "INVALID_FAMILY_ID"` (when non-UUID).
- **Notes**: Validates header syntax before querying `MembershipReader`.

### AU-04 — Non-member or soft-deleted member receives 403 `USER_NOT_IN_FAMILY`
- **Status**: `Approved`
- **Origin**: `adr:0011`
- **Given**: Valid `UserIdentity` and valid UUID `X-Family-Id: <F>`, where `MembershipReader` has no active (`deleted_at IS NULL`) membership row for `(F, user_id)` (including when the member was soft-deleted).
- **When**: Caller invokes any family-scoped route on family `F`.
- **Then**: Responds with `403 Forbidden`, `Content-Type: application/problem+json`, and `code == "USER_NOT_IN_FAMILY"`.
- **Notes**: Preserves strict multi-tenant isolation.

### AU-05 — Route `{id}` mismatching verified `X-Family-Id` returns 403 `FAMILY_ID_MISMATCH`
- **Status**: `Approved`
- **Origin**: `adr:0011`
- **Given**: Valid `UserIdentity` who is an active `Owner` of Family `A`, sending `X-Family-Id: A`.
- **When**: Caller invokes `GET /families/B` (or any `/families/B*` family-scoped route) where `B != A`.
- **Then**: Responds with `403 Forbidden`, `Content-Type: application/problem+json`, and `code == "FAMILY_ID_MISMATCH"` without reading or mutating Family `B`.
- **Notes**: Prevents URL-path tenant spoofing even when `X-Family-Id` is valid.

### AU-06 — Multi-family caller switches workspace cleanly via `X-Family-Id`
- **Status**: `Approved`
- **Origin**: `adr:0011`
- **Given**: A single `UserIdentity` who is active `Role::Owner` in Family `A` and active `Role::Child` in Family `B`.
- **When**: Caller invokes `POST /families/A/invites` with `X-Family-Id: A`, and subsequently invokes `POST /families/B/invites` with `X-Family-Id: B`.
- **Then**: Request to Family `A` resolves `Role::Owner` and succeeds (`201 Created`), while request to Family `B` resolves `Role::Child` and returns `403 Forbidden` (`code == "INSUFFICIENT_ROLE"`).
- **Notes**: Proves per-request role isolation across workspaces.

### AU-07 — Any active member can read workspace details and active member roster
- **Status**: `Approved`
- **Origin**: `human`
- **Given**: Valid `UserIdentity` with active membership in Family `A` under any role (`Owner`, `Member`, `Child`, or `Guest` with `permissions = []`) and `X-Family-Id: A`.
- **When**: Caller invokes `GET /families/A` or `GET /families/A/members`.
- **Then**: Responds with `200 OK` containing only active (`deleted_at IS NULL`) members for Family `A`.
- **Notes**: Allows workspace switchers and member name display across all member roles.

### AU-08 — Non-Owner receives 403 `INSUFFICIENT_ROLE` on family management routes
- **Status**: `Approved`
- **Origin**: `human`
- **Given**: Valid `UserIdentity` with active `Role::Member`, `Role::Child`, or `Role::Guest` (even with all 8 `Permission` flags) in Family `A`, sending `X-Family-Id: A`.
- **When**: Caller invokes `PUT /families/A`, `POST /families/A/invites`, `PATCH /families/A/members/{mid}/role`, or `DELETE /families/A/members/{mid}`.
- **Then**: Responds with `403 Forbidden`, `Content-Type: application/problem+json`, and `code == "INSUFFICIENT_ROLE"`.
- **Notes**: Enforces Owner-only family governance end-to-end at Layer 2b.

### AU-09 — Verified JWT claims require valid `sub`, supported `token_use`, and verified email for ID tokens
- **Status**: `Approved`
- **Origin**: `human`
- **Given**: Verified JWT claims map from API Gateway or `LocalJwtVerifier`.
- **When**: `UserIdentity::from_claim_map` parses the claims.
- **Then**:
  - `sub` missing/non-UUID, or `token_use` missing or not `"access"` / `"id"`, rejects with HTTP `401 Unauthorized` (`application/problem+json`, `code == "INVALID_CLAIMS"`). There is no legacy tolerance for a missing `token_use`.
  - `token_use == "access"` with valid `sub` succeeds with `email()` equal to `None` (any unverified `email` attribute is ignored).
  - `token_use == "id"` with `email_verified` of `"true"` / `true` and a valid `email` succeeds with `email() == Some(&Email)`.
  - `token_use == "id"` with `email_verified` missing, `"false"`, or `false` rejects with `401` (`code == "UNVERIFIED_EMAIL"`); a missing or invalid `email` rejects with `401` (`code == "INVALID_CLAIMS"`).
- **Notes**: Replaces main's pre-merge `AU-02`..`AU-04` (Option 2B). Fixtures for `AU-01`..`AU-08` supply `token_use` and `email_verified`.

### AU-10 — Local JWT verifier fails closed and API responses carry security headers
- **Status**: `Approved`
- **Origin**: `human`
- **Given**: Local adapter (`LocalJwtVerifier`) with a missing, malformed, expired, wrong-issuer, or wrong-audience bearer token; or any API / `ApiError` response.
- **When**: Auth middleware runs or a handler/error responds.
- **Then**:
  - Missing bearer token returns `401 Unauthorized` (`application/problem+json`, `code == "MISSING_CLAIMS"`); malformed, expired, wrong-issuer, or wrong-audience bearer tokens return `401 Unauthorized` (`code == "INVALID_CLAIMS"`); identity is never read from raw headers such as `X-User-Id`.
  - Responses include `X-Content-Type-Options: nosniff` and `Cache-Control: no-store`.
- **Notes**: Replaces main's pre-merge `AU-01` local-adapter clause and `AU-08` header clause.

### AU-11 — `AuthClaims` resolves caller `member_id` and `GET /profile/families` includes `member_id` and `permissions`
- **Status**: `Approved`
- **Origin**: `adr:0011`
- **Given**: Valid `UserIdentity` with active membership (`MemberId = M`, `Role::Guest`, `permissions = [ViewAllAccounts, RecordOwnTransactions]`) in Family `F`.
- **When**: Caller invokes `GET /profile/families` (identity-only) and any family-scoped route with `X-Family-Id: F`.
- **Then**:
  - `GET /profile/families` returns `200 OK` where the matching workspace entry includes `membership.member_id == M`, `membership.role == "guest"`, and `membership.permissions == ["view_all_accounts", "record_own_transactions"]`.
  - `family_auth_middleware` populates `AuthClaims.member_id == M` alongside `family_id`, `role`, and `permissions`.
- **Notes**: Enables zero-extra-roundtrip ownership and permission checks in `accounts_service`, `ledger_service`, and `frontend-web/`.

### AU-12 — `Role::Guest` without account or ledger capability flags receives 403 `INSUFFICIENT_ROLE`
- **Status**: `Approved`
- **Origin**: `human`
- **Given**: Active `Role::Guest` member in Family `F` whose `permissions` is `[Permission::ViewBudget]` (neither `ViewAllAccounts` nor `ManageOwnAccounts`, and none of `ViewAllTransactions`, `RecordOwnTransactions`, `RecordAnyTransaction`).
- **When**: Caller invokes `GET /accounts`, `POST /accounts`, `GET /categories`, `GET /transactions`, or `POST /transactions` with `X-Family-Id: F`.
- **Then**: Every call responds with `403 Forbidden`, `Content-Type: application/problem+json`, and `code == "INSUFFICIENT_ROLE"`.
- **Notes**: Prevents unprivileged `Guest` members from enumerating empty lists on unauthorized bounded contexts.

---

## Open Questions

*(None)*

---

## Findings

| # | Location | Code Says | Doc / ADR Says | Impact |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `backend/api/src/auth.rs` | Separates `UserIdentity` (API Gateway JWT claims) from `AuthClaims` (`X-Family-Id` + `MembershipReader` + `FAMILY_ROUTE_POLICIES` table) | `AU-01`–`AU-08` (`Status: Approved`); `AU-09`–`AU-10` (`Draft`) | Implemented in Slice 0; covered by `api/tests/au_multi_family_auth.rs` |
