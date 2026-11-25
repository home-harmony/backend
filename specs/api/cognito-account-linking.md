# IDP — Cognito `PreSignUp` & Safe Google Account Linking

- **Prefix**: `IDP`
- **Layer**: `api`
- **Unit under test**: `cognito_pre_signup::policy::evaluate_pre_signup`
- **Test seam**: `Layer 2b (Axum oneshot + fakes)`
- **Format**: `T` (Case Table)
- **Test target**: `lambdas/cognito_pre_signup/tests/idp_account_linking.rs`

---

## Vocabulary

| Term | Meaning / API Symbol | Concrete Representation |
| :--- | :--- | :--- |
| `PreSignUpInput` | `cognito_pre_signup::policy::PreSignUpInput` | Pure representation of Cognito `PreSignUp` event (`user_pool_id`, `trigger_source`, `user_name`, `email`, `email_verified`) |
| `PreSignUpDecision` | `cognito_pre_signup::policy::PreSignUpDecision` | Outcome flags (`auto_confirm_user: bool`, `auto_verify_email: bool`, `auto_verify_phone: bool`) |
| `CognitoIdentityAdmin` | `cognito_pre_signup::policy::CognitoIdentityAdmin` | RPITIT port abstracting `list_users_by_email`, `delete_unconfirmed_user`, and `link_google_to_native_user` |
| `PreSignUpError` | `cognito_pre_signup::policy::PreSignUpError` | Typed rejection errors (`InvalidEmail`, `UnverifiedFederatedEmail`, `AccountAlreadyExistsWithProvider`, `Upstream`) |

---

## Rules

| ID | Status | Origin | Given State | Action / Input | Expected Result / Next State |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `IDP-01` | `Draft` | `human` | Any Cognito pool state | `PreSignUp_ExternalProvider` with `email_verified != "true"` or invalid `Email` | Returns `Err(PreSignUpError::UnverifiedFederatedEmail | InvalidEmail)`; zero deletes or links executed |
| `IDP-02` | `Draft` | `human` | No existing Cognito users for normalized `email` | `PreSignUp_ExternalProvider` (`user_name = "Google_123"`, `email_verified = "true"`) | Returns `Ok(PreSignUpDecision { auto_confirm_user: true, auto_verify_email: true, .. })` |
| `IDP-03` | `Draft` | `human` | Existing `UNCONFIRMED` native Cognito user (`alice_stub`) for normalized `email` | `PreSignUp_ExternalProvider` (`user_name = "Google_123"`, `email_verified = "true"`) | Calls `delete_unconfirmed_user("alice_stub")`, never calls `link_google_to_native_user`, returns `auto_confirm_user = true`, `auto_verify_email = true` |
| `IDP-04` | `Draft` | `human` | Existing `CONFIRMED` native user (`sub = U1`) with `email_verified = true` and no Google identity linked | `PreSignUp_ExternalProvider` (`user_name = "Google_123"`, `email_verified = "true"`) | Calls `link_google_to_native_user(native: "U1", provider: "Google", subject: "123")`; returns `auto_confirm_user = false`, `auto_verify_email = false` |
| `IDP-05` | `Draft` | `human` | Existing `CONFIRMED` user (`sub = U1`) already linked to `Google` identity `123` | `PreSignUp_ExternalProvider` (`user_name = "Google_123"`, `email_verified = "true"`) | Skips `link_google_to_native_user` idempotently; returns `auto_confirm_user = false`, `auto_verify_email = false` |
| `IDP-06` | `Draft` | `human` | Existing `EXTERNAL_PROVIDER` (or `CONFIRMED`) user for normalized `email` | `PreSignUp_SignUp` (native Email+Password registration) | Returns `Err(PreSignUpError::AccountAlreadyExistsWithProvider)` preventing unverified takeover or duplicate shadow `sub` |
| `IDP-07` | `Draft` | `human` | Any Cognito pool state | `PreSignUp_AdminCreateUser` | Returns `Ok(PreSignUpDecision { auto_confirm_user: false, auto_verify_email: false, auto_verify_phone: false })` with zero admin mutations |

---

## Open Questions

*(None)*
