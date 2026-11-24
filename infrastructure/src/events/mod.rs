//! Domain event publisher adapters (Amazon EventBridge and in-memory test spy).

pub mod eventbridge_publisher;
#[cfg(any(test, feature = "test-utils"))]
pub mod spy_publisher;

pub use eventbridge_publisher::EventBridgePublisher;
#[cfg(any(test, feature = "test-utils"))]
pub use spy_publisher::SpyPublisher;
