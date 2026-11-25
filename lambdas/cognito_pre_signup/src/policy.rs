//! Pure decision policy for Cognito `PreSignUp` safe Google account linking (`IDP-01`..`IDP-07`).

use std::future::Future;

use domain::value_objects::Email;

const GOOGLE_PROVIDER_NAME: &str = "Google";
const GOOGLE_USERNAME_PREFIX: &str = "Google_";

/// Supported Cognito `PreSignUp` trigger sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    SignUp,
    ExternalProvider,
    AdminCreateUser,
}

/// Pure input extracted from a Cognito `PreSignUp` trigger event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreSignUpInput {
    pub user_pool_id: String,
    pub trigger_kind: TriggerKind,
    pub user_name: String,
    pub email: Option<String>,
    pub email_verified: Option<String>,
}

/// Decision returned to Cognito in `event.response`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreSignUpDecision {
    pub auto_confirm_user: bool,
    pub auto_verify_email: bool,
    pub auto_verify_phone: bool,
}

/// Minimal Cognito user projection used by [`evaluate_pre_signup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingCognitoUser {
    pub username: String,
    pub is_confirmed: bool,
    pub is_external_provider: bool,
    pub email_verified: bool,
    pub linked_google_subjects: Vec<String>,
}

/// Errors that reject registration or surface Cognito admin failures.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PreSignUpError {
    #[error("Invalid email address in registration request")]
    InvalidEmail,

    #[error("Federated identity provider must supply a verified email address")]
    UnverifiedFederatedEmail,

    #[error("Invalid external provider username format")]
    InvalidExternalUsername,

    #[error("An account with this email already exists; sign in with your linked provider")]
    AccountAlreadyExistsWithProvider,

    #[error("Cognito identity administration error: {0}")]
    Upstream(String),
}

/// RPITIT port abstracting Cognito User Pool admin calls for unit-testable `PreSignUp` policy.
pub trait CognitoIdentityAdmin: Send + Sync {
    fn list_users_by_email(
        &self,
        user_pool_id: &str,
        email: &Email,
    ) -> impl Future<Output = Result<Vec<ExistingCognitoUser>, PreSignUpError>> + Send;

    fn delete_unconfirmed_user(
        &self,
        user_pool_id: &str,
        username: &str,
    ) -> impl Future<Output = Result<(), PreSignUpError>> + Send;

    fn link_google_to_native_user(
        &self,
        user_pool_id: &str,
        native_username: &str,
        google_subject: &str,
    ) -> impl Future<Output = Result<(), PreSignUpError>> + Send;
}

/// Evaluates a Cognito `PreSignUp` event and executes any required stub deletion or linking.
pub async fn evaluate_pre_signup<A: CognitoIdentityAdmin>(
    admin: &A,
    input: &PreSignUpInput,
) -> Result<PreSignUpDecision, PreSignUpError> {
    match input.trigger_kind {
        TriggerKind::AdminCreateUser => Ok(PreSignUpDecision::default()),
        TriggerKind::SignUp => handle_native_signup(admin, input).await,
        TriggerKind::ExternalProvider => handle_external_provider(admin, input).await,
    }
}

/// Returns the Cognito provider name expected by `AdminLinkProviderForUser`.
#[must_use]
pub const fn google_provider_name() -> &'static str {
    GOOGLE_PROVIDER_NAME
}

async fn handle_native_signup<A: CognitoIdentityAdmin>(
    admin: &A,
    input: &PreSignUpInput,
) -> Result<PreSignUpDecision, PreSignUpError> {
    let email = parse_email_attr(input.email.as_deref())?;
    let existing = admin
        .list_users_by_email(&input.user_pool_id, &email)
        .await?;

    let conflict = existing
        .iter()
        .any(|u| u.is_external_provider || u.is_confirmed);
    if conflict {
        return Err(PreSignUpError::AccountAlreadyExistsWithProvider);
    }

    Ok(PreSignUpDecision::default())
}

async fn handle_external_provider<A: CognitoIdentityAdmin>(
    admin: &A,
    input: &PreSignUpInput,
) -> Result<PreSignUpDecision, PreSignUpError> {
    if !is_email_verified(input.email_verified.as_deref()) {
        return Err(PreSignUpError::UnverifiedFederatedEmail);
    }

    let email = parse_email_attr(input.email.as_deref())?;
    let google_subject = parse_google_subject(&input.user_name)?;
    let existing = admin
        .list_users_by_email(&input.user_pool_id, &email)
        .await?;

    purge_unconfirmed_stubs(admin, &input.user_pool_id, &existing).await?;

    let Some(confirmed_user) = existing.iter().find(|u| u.is_confirmed && u.email_verified) else {
        return Ok(PreSignUpDecision {
            auto_confirm_user: true,
            auto_verify_email: true,
            auto_verify_phone: false,
        });
    };

    link_google_if_absent(admin, &input.user_pool_id, confirmed_user, google_subject).await?;
    // Critical Cognito invariant: leave flags false after AdminLinkProviderForUser.
    Ok(PreSignUpDecision::default())
}

async fn link_google_if_absent<A: CognitoIdentityAdmin>(
    admin: &A,
    user_pool_id: &str,
    confirmed_user: &ExistingCognitoUser,
    google_subject: &str,
) -> Result<(), PreSignUpError> {
    let already_linked = confirmed_user
        .linked_google_subjects
        .iter()
        .any(|sub| sub == google_subject);
    if !already_linked {
        admin
            .link_google_to_native_user(user_pool_id, &confirmed_user.username, google_subject)
            .await?;
    }
    Ok(())
}

fn is_email_verified(flag: Option<&str>) -> bool {
    flag.is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

async fn purge_unconfirmed_stubs<A: CognitoIdentityAdmin>(
    admin: &A,
    user_pool_id: &str,
    existing: &[ExistingCognitoUser],
) -> Result<(), PreSignUpError> {
    for user in existing {
        if !user.is_confirmed && !user.is_external_provider {
            admin
                .delete_unconfirmed_user(user_pool_id, &user.username)
                .await?;
        }
    }
    Ok(())
}

fn parse_email_attr(raw: Option<&str>) -> Result<Email, PreSignUpError> {
    let text = raw.ok_or(PreSignUpError::InvalidEmail)?;
    Email::try_new(text).map_err(|_| PreSignUpError::InvalidEmail)
}

fn parse_google_subject(user_name: &str) -> Result<&str, PreSignUpError> {
    user_name
        .strip_prefix(GOOGLE_USERNAME_PREFIX)
        .filter(|s| !s.is_empty())
        .ok_or(PreSignUpError::InvalidExternalUsername)
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct FakeCognitoAdmin {
        users: Vec<ExistingCognitoUser>,
        deleted: Mutex<Vec<String>>,
        linked: Mutex<Vec<(String, String)>>,
    }

    impl CognitoIdentityAdmin for FakeCognitoAdmin {
        async fn list_users_by_email(
            &self,
            _user_pool_id: &str,
            _email: &Email,
        ) -> Result<Vec<ExistingCognitoUser>, PreSignUpError> {
            Ok(self.users.clone())
        }

        async fn delete_unconfirmed_user(
            &self,
            _user_pool_id: &str,
            username: &str,
        ) -> Result<(), PreSignUpError> {
            self.deleted.lock().unwrap().push(username.to_owned());
            Ok(())
        }

        async fn link_google_to_native_user(
            &self,
            _user_pool_id: &str,
            native_username: &str,
            google_subject: &str,
        ) -> Result<(), PreSignUpError> {
            self.linked
                .lock()
                .unwrap()
                .push((native_username.to_owned(), google_subject.to_owned()));
            Ok(())
        }
    }

    fn google_input(email_verified: Option<&str>) -> PreSignUpInput {
        PreSignUpInput {
            user_pool_id: "us-east-1_Pool".to_string(),
            trigger_kind: TriggerKind::ExternalProvider,
            user_name: "Google_123456".to_string(),
            email: Some("  Alice@Gmail.COM ".to_string()),
            email_verified: email_verified.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn idp_01_rejects_unverified_or_invalid_federated_email() {
        let admin = FakeCognitoAdmin::default();
        assert_eq!(
            evaluate_pre_signup(&admin, &google_input(Some("false")))
                .await
                .unwrap_err(),
            PreSignUpError::UnverifiedFederatedEmail
        );
        assert_eq!(
            evaluate_pre_signup(&admin, &google_input(None))
                .await
                .unwrap_err(),
            PreSignUpError::UnverifiedFederatedEmail
        );
        assert!(admin.deleted.lock().unwrap().is_empty());
        assert!(admin.linked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn idp_02_fresh_google_user_auto_confirms_and_verifies() {
        let admin = FakeCognitoAdmin::default();
        let decision = evaluate_pre_signup(&admin, &google_input(Some("true")))
            .await
            .unwrap();
        assert_eq!(
            decision,
            PreSignUpDecision {
                auto_confirm_user: true,
                auto_verify_email: true,
                auto_verify_phone: false,
            }
        );
        assert!(admin.deleted.lock().unwrap().is_empty());
        assert!(admin.linked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn idp_03_unconfirmed_native_stub_is_deleted_never_linked() {
        let admin = FakeCognitoAdmin {
            users: vec![ExistingCognitoUser {
                username: "alice_stub".to_string(),
                is_confirmed: false,
                is_external_provider: false,
                email_verified: false,
                linked_google_subjects: Vec::new(),
            }],
            ..Default::default()
        };

        let decision = evaluate_pre_signup(&admin, &google_input(Some("true")))
            .await
            .unwrap();
        assert_eq!(
            decision,
            PreSignUpDecision {
                auto_confirm_user: true,
                auto_verify_email: true,
                auto_verify_phone: false,
            }
        );
        assert_eq!(*admin.deleted.lock().unwrap(), vec!["alice_stub"]);
        assert!(admin.linked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn idp_04_confirmed_native_user_links_google_with_false_confirm_flags() {
        let admin = FakeCognitoAdmin {
            users: vec![ExistingCognitoUser {
                username: "native-u1".to_string(),
                is_confirmed: true,
                is_external_provider: false,
                email_verified: true,
                linked_google_subjects: Vec::new(),
            }],
            ..Default::default()
        };

        let decision = evaluate_pre_signup(&admin, &google_input(Some("true")))
            .await
            .unwrap();
        assert_eq!(decision, PreSignUpDecision::default());
        assert_eq!(
            *admin.linked.lock().unwrap(),
            vec![("native-u1".to_string(), "123456".to_string())]
        );
    }

    #[tokio::test]
    async fn idp_05_already_linked_google_subject_is_idempotent() {
        let admin = FakeCognitoAdmin {
            users: vec![ExistingCognitoUser {
                username: "native-u1".to_string(),
                is_confirmed: true,
                is_external_provider: false,
                email_verified: true,
                linked_google_subjects: vec!["123456".to_string()],
            }],
            ..Default::default()
        };

        let decision = evaluate_pre_signup(&admin, &google_input(Some("true")))
            .await
            .unwrap();
        assert_eq!(decision, PreSignUpDecision::default());
        assert!(admin.linked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn idp_06_native_signup_blocked_when_external_or_confirmed_user_exists() {
        let admin = FakeCognitoAdmin {
            users: vec![ExistingCognitoUser {
                username: "Google_123456".to_string(),
                is_confirmed: true,
                is_external_provider: true,
                email_verified: true,
                linked_google_subjects: vec!["123456".to_string()],
            }],
            ..Default::default()
        };

        let input = PreSignUpInput {
            user_pool_id: "us-east-1_Pool".to_string(),
            trigger_kind: TriggerKind::SignUp,
            user_name: "alice_native".to_string(),
            email: Some("alice@gmail.com".to_string()),
            email_verified: None,
        };
        assert_eq!(
            evaluate_pre_signup(&admin, &input).await.unwrap_err(),
            PreSignUpError::AccountAlreadyExistsWithProvider
        );
    }

    #[tokio::test]
    async fn idp_07_admin_create_user_passes_through_untouched() {
        let admin = FakeCognitoAdmin::default();
        let input = PreSignUpInput {
            user_pool_id: "us-east-1_Pool".to_string(),
            trigger_kind: TriggerKind::AdminCreateUser,
            user_name: "admin-created".to_string(),
            email: Some("alice@gmail.com".to_string()),
            email_verified: None,
        };
        let decision = evaluate_pre_signup(&admin, &input).await.unwrap();
        assert_eq!(decision, PreSignUpDecision::default());
        assert!(admin.deleted.lock().unwrap().is_empty());
        assert!(admin.linked.lock().unwrap().is_empty());
    }
}
