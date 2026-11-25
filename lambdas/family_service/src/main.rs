//! AWS Lambda entrypoint for `family_service`.

use std::sync::Arc;

use tower::ServiceBuilder;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use api::auth::strip_apigw_stage_prefix;
use family_service::{AppState, build_router};
use infrastructure::{
    db::pool::create_service_pool, events::EventBridgePublisher, repositories::DsqlFamilyRepository,
};

const EVENT_SOURCE: &str = "familyledger.family_service";

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().json())
        .init();

    let pool = create_service_pool().await?;
    let repo = Arc::new(DsqlFamilyRepository::new(pool));
    let publisher = Arc::new(EventBridgePublisher::from_env(EVENT_SOURCE).await?);
    let app = ServiceBuilder::new()
        .map_request(strip_apigw_stage_prefix)
        .service(build_router(AppState::new(repo, publisher)));

    lambda_http::run(app).await
}
