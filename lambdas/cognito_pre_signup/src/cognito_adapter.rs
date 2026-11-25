//! AWS SDK adapter for [`CognitoIdentityAdmin`].

use aws_sdk_cognitoidentityprovider::{
    Client,
    types::{ProviderUserIdentifierType, UserStatusType, UserType},
};
use domain::value_objects::Email;

use crate::policy::{
    CognitoIdentityAdmin, ExistingCognitoUser, PreSignUpError, google_provider_name,
};

/// AWS SDK implementation of [`CognitoIdentityAdmin`].
#[derive(Clone)]
pub struct AwsCognitoIdentityAdmin {
    client: Client,
}

impl AwsCognitoIdentityAdmin {
    /// Wraps an initialized Cognito Identity Provider SDK client.
    #[must_use]
    pub const fn new(client: Client) -> Self {
        Self { client }
    }
}

impl CognitoIdentityAdmin for AwsCognitoIdentityAdmin {
    async fn list_users_by_email(
        &self,
        user_pool_id: &str,
        email: &Email,
    ) -> Result<Vec<ExistingCognitoUser>, PreSignUpError> {
        let filter = format!("email = \"{}\"", email.as_str());
        let resp = self
            .client
            .list_users()
            .user_pool_id(user_pool_id)
            .filter(filter)
            .limit(10)
            .send()
            .await
            .map_err(|e| PreSignUpError::Upstream(e.to_string()))?;

        Ok(resp.users().iter().filter_map(map_user_type).collect())
    }

    async fn delete_unconfirmed_user(
        &self,
        user_pool_id: &str,
        username: &str,
    ) -> Result<(), PreSignUpError> {
        self.client
            .admin_delete_user()
            .user_pool_id(user_pool_id)
            .username(username)
            .send()
            .await
            .map_err(|e| PreSignUpError::Upstream(e.to_string()))?;
        Ok(())
    }

    async fn link_google_to_native_user(
        &self,
        user_pool_id: &str,
        native_username: &str,
        google_subject: &str,
    ) -> Result<(), PreSignUpError> {
        let destination = ProviderUserIdentifierType::builder()
            .provider_name("Cognito")
            .provider_attribute_value(native_username)
            .build();
        let source = ProviderUserIdentifierType::builder()
            .provider_name(google_provider_name())
            .provider_attribute_name("Cognito_Subject")
            .provider_attribute_value(google_subject)
            .build();

        self.client
            .admin_link_provider_for_user()
            .user_pool_id(user_pool_id)
            .destination_user(destination)
            .source_user(source)
            .send()
            .await
            .map_err(|e| PreSignUpError::Upstream(e.to_string()))?;
        Ok(())
    }
}

fn map_user_type(user: &UserType) -> Option<ExistingCognitoUser> {
    let username = user.username()?.to_owned();
    let status = user.user_status();
    let is_confirmed = matches!(status, Some(UserStatusType::Confirmed));
    let is_external_provider = matches!(status, Some(UserStatusType::ExternalProvider));
    let email_verified = user
        .attributes()
        .iter()
        .find(|a| a.name() == "email_verified")
        .and_then(|a| a.value())
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let linked_google_subjects = extract_google_subjects(user);

    Some(ExistingCognitoUser {
        username,
        is_confirmed,
        is_external_provider,
        email_verified,
        linked_google_subjects,
    })
}

fn extract_google_subjects(user: &UserType) -> Vec<String> {
    let Some(raw) = user
        .attributes()
        .iter()
        .find(|a| a.name() == "identities")
        .and_then(|a| a.value())
    else {
        return Vec::new();
    };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter(|e| e.get("providerName").and_then(|v| v.as_str()) == Some(google_provider_name()))
        .filter_map(|e| e.get("userId").and_then(|v| v.as_str()).map(str::to_owned))
        .collect()
}
