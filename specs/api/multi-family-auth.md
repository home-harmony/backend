# AU — Cognito JWT & Multi-Family `X-Family-Id` Isolation

- **Prefix**: `AU`
- **Layer**: `api`
- **Unit under test**: `api::auth::{AuthClaims, AuthenticatedUser, VerifiedJwtIdentity, resolve_family_auth_claims}`
- **Test seam**: `Layer 2b (Axum oneshot + fakes)`
- **Format**: `U` (Use Case)
- **Test target**: `api/tests/au_multi_family_auth.rs`

---

## Vocabulary

| Term | Meaning / API Symbol | Concrete Representation |
| :--- | :--- | :--- |
| `AuthenticatedUser` | `api::auth::AuthenticatedUser` | Sealed authenticated user identity (`user_id() -> UserId`, `email() -> Option<&Email>`) for non-family routes |
| `AuthClaims` | `api::auth::AuthClaims` | Sealed family-scoped claims (`user_id()`, `family_id()`, `role()`, `permissions()`, `email()`) with private fields and no `Deserialize` |
| `VerifiedJwtIdentity` | `api::auth::VerifiedJwtIdentity` | Parsed and semantically validated JWT claims (`sub`, `token_use`, `email_verified`, `email`) from API Gateway or `LocalJwtVerifier` |
| `FamilyMembershipReader` | `domain::ports::FamilyMembershipReader` | RPITIT domain port resolving active `(family_id, user_id)` membership (`deleted_at IS NULL`) |
| `AuthError` | `api::auth::AuthError` | RFC 9457 `application/problem+json` error variants (`MissingClaims`, `InvalidClaims`, `UnverifiedEmail`, `MissingFamilyId`, `InvalidFamilyId`, `Forbidden`) |

---

## Rules

### AU-01 — Missing authorizer or JWT claims fails closed with 401 MISSING_CLAIMS
- **Status**: `Draft`
- **Origin**: `adr:0011`
- **Given**: An incoming HTTP request with no API Gateway JWT authorizer context (and no verified local JWT when using the local adapter), even if raw `X-User-Id` or unverified headers are present.
- **When**: Auth middleware or `AuthenticatedUser` / `AuthClaims` extraction runs.
- **Then**: Returns HTTP `401 Unauthorized` (`Content-Type: application/problem+json`) with `code == "MISSING_CLAIMS"` and never reads identity from raw HTTP headers.

### AU-02 — Malformed sub UUID or unsupported token_use fails with 401 INVALID_CLAIMS
- **Status**: `Draft`
- **Origin**: `human`
- **Given**: Verified JWT claims map where `sub` is missing/non-UUID or `token_use` is missing or not `"access"` / `"id"`.
- **When**: `VerifiedJwtIdentity::from_claim_map` parses the claims.
- **Then**: Returns HTTP `401 Unauthorized` (`application/problem+json`) with `code == "INVALID_CLAIMS"`.

### AU-03 — Valid Cognito Access Token resolves AuthenticatedUser with email None
- **Status**: `Draft`
- **Origin**: `adr:0011`
- **Given**: Verified JWT claims map with valid UUID `sub` and `token_use == "access"` (native or Google-federated user).
- **When**: `VerifiedJwtIdentity::from_claim_map` runs.
- **Then**: Succeeds with `AuthenticatedUser` where `user_id()` matches `sub` and `email()` is `None` (ignoring any unverified `email` attribute).

### AU-04 — Valid Cognito ID Token requires email_verified == true and valid Email
- **Status**: `Draft`
- **Origin**: `human`
- **Given**: Verified JWT claims map with valid UUID `sub` and `token_use == "id"`.
- **When**: `VerifiedJwtIdentity::from_claim_map` runs.
- **Then**:
  - If `email_verified` is `"true"` or `true` and `email` parses as `Email`, succeeds with `email() == Some(&Email)`.
  - If `email_verified` is missing, `"false"`, or `false`, rejects with HTTP `401 Unauthorized` (`code == "UNVERIFIED_EMAIL"`).
  - If `email` is missing or fails `Email` validation, rejects with HTTP `401 Unauthorized` (`code == "INVALID_CLAIMS"`).

### AU-05 — Non-family routes using AuthenticatedUser succeed without X-Family-Id
- **Status**: `Draft`
- **Origin**: `adr:0011`
- **Given**: A valid `VerifiedJwtIdentity` inserted as `AuthenticatedUser` on an onboarding or profile route (e.g., `GET /profile/families`, `POST /families`, `POST /families/{id}/members`) with no `X-Family-Id` header.
- **When**: Handler extracts `AuthenticatedUser`.
- **Then**: Extraction succeeds without querying `family_members`.

### AU-06 — Family-scoped routes reject missing or malformed X-Family-Id with 400
- **Status**: `Draft`
- **Origin**: `adr:0011`
- **Given**: A valid `VerifiedJwtIdentity` on a family-scoped route.
- **When**: `resolve_family_auth_claims` inspects the `X-Family-Id` HTTP header.
- **Then**:
  - Missing or empty `X-Family-Id` returns HTTP `400 Bad Request` (`code == "MISSING_FAMILY_ID"`).
  - Non-UUID `X-Family-Id` returns HTTP `400 Bad Request` (`code == "INVALID_FAMILY_ID"`).

### AU-07 — Non-member or soft-deleted member is rejected with 403 USER_NOT_IN_FAMILY
- **Status**: `Draft`
- **Origin**: `adr:0011`
- **Given**: A valid `VerifiedJwtIdentity` and syntactically valid `X-Family-Id`, where `FamilyMembershipReader` returns `Ok(None)` (user is not a member or has `deleted_at IS NOT NULL`).
- **When**: `resolve_family_auth_claims` checks membership.
- **Then**: Returns HTTP `403 Forbidden` (`application/problem+json`) with `code == "USER_NOT_IN_FAMILY"` and does not insert `AuthClaims`.

### AU-08 — Active member resolves sealed AuthClaims and responses carry security headers
- **Status**: `Draft`
- **Origin**: `human`
- **Given**: A valid `VerifiedJwtIdentity`, valid `X-Family-Id`, and active membership returned by `FamilyMembershipReader`.
- **When**: `resolve_family_auth_claims` runs and a handler/error responds.
- **Then**:
  - Inserts sealed `AuthClaims` (`user_id()`, `family_id()`, `role()`, `permissions()`, `email()`) into request extensions; `AuthClaims` cannot be deserialized from untrusted JSON.
  - API responses and `ApiError` responses include `X-Content-Type-Options: nosniff` and `Cache-Control: no-store`.

---

## Open Questions

*(None)*
