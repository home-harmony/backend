//! Domain events — immutable records of things that happened in the domain.
//!
//! These events are emitted by aggregate methods and consumed by:
//! - The infrastructure layer (to publish to EventBridge via CDC/Kinesis)
//! - Other aggregates within the same transaction (for cross-context consistency)
//!
//! Events carry only the data needed to describe what happened — no logic.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value_objects::Role;

/// All domain events emitted across all bounded contexts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum DomainEvent {
    // ── Identity & Family context ─────────────────────────────────────────────
    FamilyCreated {
        family_id: Uuid,
        name: String,
        home_currency: String,
        occurred_at: DateTime<Utc>,
    },
    MemberInvited {
        family_id: Uuid,
        invite_token: String,
        role: Role,
        created_by: Uuid,
        occurred_at: DateTime<Utc>,
    },
    MemberJoined {
        family_id: Uuid,
        member_id: Uuid,
        user_id: Uuid,
        role: Role,
        occurred_at: DateTime<Utc>,
    },
    MemberRoleChanged {
        family_id: Uuid,
        member_id: Uuid,
        old_role: Role,
        new_role: Role,
        changed_by: Uuid,
        occurred_at: DateTime<Utc>,
    },
    MemberRemoved {
        family_id: Uuid,
        member_id: Uuid,
        removed_by: Uuid,
        occurred_at: DateTime<Utc>,
    },
}

impl DomainEvent {
    /// Returns the family ID associated with this event.
    /// All events in FamilyLedger belong to a family.
    pub fn family_id(&self) -> Uuid {
        match self {
            DomainEvent::FamilyCreated { family_id, .. } => *family_id,
            DomainEvent::MemberInvited { family_id, .. } => *family_id,
            DomainEvent::MemberJoined { family_id, .. } => *family_id,
            DomainEvent::MemberRoleChanged { family_id, .. } => *family_id,
            DomainEvent::MemberRemoved { family_id, .. } => *family_id,
        }
    }

    /// Returns the timestamp when this event occurred.
    pub fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            DomainEvent::FamilyCreated { occurred_at, .. } => *occurred_at,
            DomainEvent::MemberInvited { occurred_at, .. } => *occurred_at,
            DomainEvent::MemberJoined { occurred_at, .. } => *occurred_at,
            DomainEvent::MemberRoleChanged { occurred_at, .. } => *occurred_at,
            DomainEvent::MemberRemoved { occurred_at, .. } => *occurred_at,
        }
    }
}
