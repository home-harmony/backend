//! Domain event publisher adapters (Amazon EventBridge and in-memory test spy).

pub mod eventbridge_publisher;
pub mod spy_publisher;

pub use eventbridge_publisher::EventBridgePublisher;
pub use spy_publisher::SpyPublisher;
