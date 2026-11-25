//! Cognito `PreSignUp` Lambda for safe Google OAuth account linking & takeover prevention.

mod cognito_adapter;
pub mod policy;

use aws_lambda_events::event::cognito::{
    CognitoEventUserPoolsPreSignup, CognitoEventUserPoolsPreSignupTriggerSource,
};
use lambda_runtime::{Error, LambdaEvent, run, service_fn};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    cognito_adapter::AwsCognitoIdentityAdmin,
    policy::{PreSignUpInput, TriggerKind, evaluate_pre_signup},
};

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().json())
        .init();

    let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let client = aws_sdk_cognitoidentityprovider::Client::new(&sdk_config);
    let admin = AwsCognitoIdentityAdmin::new(client);

    run(service_fn(|event| handler(&admin, event))).await
}

async fn handler(
    admin: &AwsCognitoIdentityAdmin,
    event: LambdaEvent<CognitoEventUserPoolsPreSignup>,
) -> Result<CognitoEventUserPoolsPreSignup, Error> {
    let mut payload = event.payload;
    let input = build_policy_input(&payload)?;
    let decision = evaluate_pre_signup(admin, &input).await?;

    payload.response.auto_confirm_user = decision.auto_confirm_user;
    payload.response.auto_verify_email = decision.auto_verify_email;
    payload.response.auto_verify_phone = decision.auto_verify_phone;
    Ok(payload)
}

fn build_policy_input(
    payload: &CognitoEventUserPoolsPreSignup,
) -> Result<PreSignUpInput, Error> {
    let user_pool_id = payload
        .cognito_event_user_pools_header
        .user_pool_id
        .clone()
        .ok_or_else(|| Error::from("Missing userPoolId in Cognito PreSignUp event"))?;
    let user_name = payload
        .cognito_event_user_pools_header
        .user_name
        .clone()
        .ok_or_else(|| Error::from("Missing userName in Cognito PreSignUp event"))?;
    let trigger_kind = match payload.cognito_event_user_pools_header.trigger_source {
        Some(CognitoEventUserPoolsPreSignupTriggerSource::SignUp) => TriggerKind::SignUp,
        Some(CognitoEventUserPoolsPreSignupTriggerSource::ExternalProvider) => {
            TriggerKind::ExternalProvider
        }
        Some(CognitoEventUserPoolsPreSignupTriggerSource::AdminCreateUser) | None => {
            TriggerKind::AdminCreateUser
        }
        Some(_) => TriggerKind::AdminCreateUser,
    };

    Ok(PreSignUpInput {
        user_pool_id,
        trigger_kind,
        user_name,
        email: payload.request.user_attributes.get("email").cloned(),
        email_verified: payload.request.user_attributes.get("email_verified").cloned(),
    })
}
