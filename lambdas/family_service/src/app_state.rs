//! Shared Axum application state for `family_service`.

use std::sync::Arc;

/// Dependency container shared across `family_service` handlers.
pub struct AppState<R, P> {
    pub repo: Arc<R>,
    pub publisher: Arc<P>,
}

impl<R, P> AppState<R, P> {
    /// Constructs a new [`AppState`] from repository and event publisher ports.
    #[must_use]
    pub fn new(repo: Arc<R>, publisher: Arc<P>) -> Self {
        Self { repo, publisher }
    }
}

impl<R, P> Clone for AppState<R, P> {
    fn clone(&self) -> Self {
        Self {
            repo: Arc::clone(&self.repo),
            publisher: Arc::clone(&self.publisher),
        }
    }
}
