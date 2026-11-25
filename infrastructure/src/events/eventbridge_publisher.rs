//! Amazon EventBridge adapter implementing [`domain::ports::EventPublisher`].

use anyhow::{Context, Result, bail};
use aws_config::BehaviorVersion;
use aws_sdk_eventbridge::{
    Client, operation::put_events::PutEventsOutput, primitives::DateTime as AwsDateTime,
    types::PutEventsRequestEntry,
};

use domain::{errors::DomainError, events::DomainEvent, ports::EventPublisher};

const EVENTBRIDGE_MAX_BATCH_SIZE: usize = 10;

/// Publishes committed [`DomainEvent`] batches to Amazon EventBridge via `PutEvents`.
#[derive(Debug, Clone)]
pub struct EventBridgePublisher {
    client: Client,
    event_bus_name: String,
    source: String,
}

impl EventBridgePublisher {
    /// Creates a publisher using an existing EventBridge SDK client.
    pub fn new(
        client: Client,
        event_bus_name: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            client,
            event_bus_name: event_bus_name.into(),
            source: source.into(),
        }
    }

    /// Initializes a publisher from `EVENT_BUS_NAME` and default AWS SDK configuration.
    pub async fn from_env(source: impl Into<String>) -> Result<Self> {
        let event_bus_name = std::env::var("EVENT_BUS_NAME")
            .context("EVENT_BUS_NAME environment variable is required")?;
        let trimmed = event_bus_name.trim();
        if trimmed.is_empty() {
            bail!("EVENT_BUS_NAME environment variable must not be empty");
        }
        let sdk_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        Ok(Self::new(Client::new(&sdk_config), trimmed, source))
    }

    async fn publish_batch(&self, batch: &[DomainEvent]) -> Result<(), DomainError> {
        let entries = build_entries_batch(&self.event_bus_name, &self.source, batch)?;
        let output = self
            .client
            .put_events()
            .set_entries(Some(entries))
            .send()
            .await
            .map_err(|err| DomainError::Internal {
                message: format!("EventBridge PutEvents failed: {err}"),
            })?;
        check_put_events_output(&output)
    }
}

impl EventPublisher for EventBridgePublisher {
    async fn publish(&self, events: &[DomainEvent]) -> Result<(), DomainError> {
        if events.is_empty() {
            return Ok(());
        }
        for chunk in events.chunks(EVENTBRIDGE_MAX_BATCH_SIZE) {
            self.publish_batch(chunk).await?;
        }
        Ok(())
    }
}

fn check_put_events_output(output: &PutEventsOutput) -> Result<(), DomainError> {
    if output.failed_entry_count() <= 0 {
        return Ok(());
    }
    let failed_details: Vec<String> = output
        .entries()
        .iter()
        .filter_map(|entry| {
            let code = entry.error_code()?;
            let msg = entry.error_message().unwrap_or("unknown error");
            Some(format!("{code}: {msg}"))
        })
        .collect();
    Err(DomainError::Internal {
        message: format!(
            "EventBridge PutEvents reported {} failed entries: {}",
            output.failed_entry_count(),
            failed_details.join("; ")
        ),
    })
}

fn build_entries_batch(
    event_bus_name: &str,
    source: &str,
    batch: &[DomainEvent],
) -> Result<Vec<PutEventsRequestEntry>, DomainError> {
    batch
        .iter()
        .map(|event| build_put_events_entry(event_bus_name, source, event))
        .collect()
}

fn build_put_events_entry(
    event_bus_name: &str,
    source: &str,
    event: &DomainEvent,
) -> Result<PutEventsRequestEntry, DomainError> {
    let detail = serde_json::to_string(event).map_err(|err| DomainError::Internal {
        message: format!("Failed to serialize DomainEvent for EventBridge: {err}"),
    })?;
    let timestamp = AwsDateTime::from_millis(event.occurred_at().timestamp_millis());
    Ok(PutEventsRequestEntry::builder()
        .event_bus_name(event_bus_name)
        .source(source)
        .detail_type(event.detail_type())
        .detail(detail)
        .time(timestamp)
        .build())
}

// ─── Tests ────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use aws_sdk_eventbridge::types::PutEventsResultEntry;
    use chrono::Utc;

    use domain::value_objects::{CurrencyCode, FamilyId, FamilyName, MemberId, Role, UserId};

    use super::*;

    #[test]
    fn build_put_events_entry_populates_required_fields() {
        let event = sample_event();
        let entry =
            build_put_events_entry("family-bus", "familyledger.family_service", &event).unwrap();

        assert_eq!(entry.event_bus_name(), Some("family-bus"));
        assert_eq!(entry.source(), Some("familyledger.family_service"));
        assert_eq!(entry.detail_type(), Some("FamilyCreated"));
        assert!(entry.time().is_some());

        let detail: DomainEvent = serde_json::from_str(entry.detail().unwrap()).unwrap();
        assert_eq!(detail.family_id(), event.family_id());
    }

    #[test]
    fn build_entries_batch_preserves_batch_boundaries_of_at_most_ten() {
        let one = build_entries_batch("bus", "src", &[sample_event()]).unwrap();
        assert_eq!(one.len(), 1);

        let twenty_five = vec![
            DomainEvent::MemberJoined {
                family_id: FamilyId::new_v4(),
                member_id: MemberId::new_v4(),
                user_id: UserId::new_v4(),
                role: Role::Member,
                occurred_at: Utc::now(),
            };
            25
        ];
        let batch_sizes: Vec<usize> = twenty_five
            .chunks(EVENTBRIDGE_MAX_BATCH_SIZE)
            .map(|chunk| build_entries_batch("bus", "src", chunk).unwrap().len())
            .collect();
        assert_eq!(batch_sizes, vec![10, 10, 5]);
    }

    #[test]
    fn check_put_events_output_includes_failed_entry_details() {
        let output = PutEventsOutput::builder()
            .failed_entry_count(1)
            .entries(
                PutEventsResultEntry::builder()
                    .error_code("InternalFailure")
                    .error_message("throttled")
                    .build(),
            )
            .build();

        let err = check_put_events_output(&output).unwrap_err();
        match err {
            DomainError::Internal { message } => {
                assert!(message.contains("InternalFailure"), "got: {message}");
                assert!(message.contains("throttled"), "got: {message}");
            }
            other => panic!("expected DomainError::Internal, got {other:?}"),
        }
    }

    fn sample_event() -> DomainEvent {
        DomainEvent::FamilyCreated {
            family_id: FamilyId::new_v4(),
            name: FamilyName::try_new("Smith Family").unwrap(),
            home_currency: CurrencyCode::try_new("USD").unwrap(),
            occurred_at: Utc::now(),
        }
    }
}
