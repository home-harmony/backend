//! In-memory [`domain::ports::EventPublisher`] test spy for integration tests.

use std::sync::Arc;

use tokio::sync::Mutex;

use domain::{errors::DomainError, events::DomainEvent, ports::EventPublisher};

/// Thread-safe in-memory [`EventPublisher`] recording published events or simulating failures.
#[derive(Debug, Clone, Default)]
pub struct SpyPublisher {
    state: Arc<Mutex<SpyState>>,
}

#[derive(Debug, Default)]
struct SpyState {
    events: Vec<DomainEvent>,
    failure_reason: Option<String>,
}

impl SpyPublisher {
    /// Creates an empty recording publisher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a publisher that fails every `publish` call with `DomainError::Internal`.
    #[must_use]
    pub fn failing(reason: impl Into<String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(SpyState {
                events: Vec::new(),
                failure_reason: Some(reason.into()),
            })),
        }
    }

    /// Returns a snapshot of all events recorded so far.
    pub async fn events(&self) -> Vec<DomainEvent> {
        self.state.lock().await.events.clone()
    }

    /// Clears all recorded events.
    pub async fn clear(&self) {
        self.state.lock().await.events.clear();
    }
}

impl EventPublisher for SpyPublisher {
    async fn publish(&self, events: &[DomainEvent]) -> Result<(), DomainError> {
        let mut guard = self.state.lock().await;
        if let Some(reason) = &guard.failure_reason {
            return Err(DomainError::Internal {
                message: reason.clone(),
            });
        }
        guard.events.extend_from_slice(events);
        Ok(())
    }
}
