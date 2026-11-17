//! Domain events — immutable records of things that happened in the domain.
//!
//! These events are emitted by aggregate methods and consumed by:
//! - The infrastructure layer (published directly to Amazon EventBridge per ADR-0017)
//! - Other aggregates within the same transaction (for cross-context consistency)
//!
//! Events carry only the data needed to describe what happened — no logic.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::value_objects::{FamilyId, InviteTokenId, MemberId, Permission, Role, UserId};

/// All domain events emitted across all bounded contexts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum DomainEvent {
    // ── Sprint 1: Identity & Family context ─────────────────────────────────────
    FamilyCreated {
        family_id: FamilyId,
        name: String,
        home_currency: String,
        occurred_at: DateTime<Utc>,
    },
    MemberInvited {
        family_id: FamilyId,
        invite_token: InviteTokenId,
        role: Role,
        permissions: Vec<Permission>,
        created_by: MemberId,
        occurred_at: DateTime<Utc>,
    },
    MemberJoined {
        family_id: FamilyId,
        member_id: MemberId,
        user_id: UserId,
        role: Role,
        occurred_at: DateTime<Utc>,
    },
    MemberRoleChanged {
        family_id: FamilyId,
        member_id: MemberId,
        old_role: Role,
        new_role: Role,
        changed_by: MemberId,
        occurred_at: DateTime<Utc>,
    },
    MemberRemoved {
        family_id: FamilyId,
        member_id: MemberId,
        removed_by: MemberId,
        occurred_at: DateTime<Utc>,
    },
    // ── TODO: Sprint 3 — Payment Cards & Accounts Context ───────────────────────
    // - AccountRegistered { family_id, account_id, owner_member_id, kind, currency, occurred_at }
    // - AccountBalanceSnapshotted { family_id, account_id, year_month, closing_balance, occurred_at }

    // ── TODO: Sprint 3 — Ledger Core Context ─────────────────────────────────────
    // - TransactionRecorded { family_id, transaction_id, recorded_by, kind, amount, occurred_at }
    // - TransactionReversed { family_id, transaction_id, amendment_of_id, amount, occurred_at }
    // - TransactionDeleted { family_id, transaction_id, occurred_at }

    // ── TODO: Sprint 4 — Debt Planner Context ────────────────────────────────────
    // - LoanRegistered { family_id, loan_id, name, loan_kind_id, principal, occurred_at }
    // - LoanKindCreated { family_id, loan_kind_id, code, name, occurred_at }
    // - LoanPaymentRecorded { family_id, loan_id, payment_id, source_account_id, amount, principal_portion, interest_portion, remaining_balance, occurred_at }
    // - LoanPaymentLedgerized { family_id, payment_id, transaction_id, occurred_at }
    // - CreditCardSpendingUpdatedLoanBalance { family_id, loan_id, account_id, transaction_id, new_balance, occurred_at }
    // - LoanPaidOff { family_id, loan_id, occurred_at }

    // ── TODO: Sprint 5 — Recurring, Budget, Planning & FX Contexts ───────────────
    // - RecurringPaymentRegistered { family_id, recurring_id, name, amount, frequency, occurred_at }
    // - RecurringPaymentRecorded { family_id, recurring_id, record_id, actual_amount, paid_at, occurred_at }
    // - RecurringPaymentMissed { family_id, recurring_id, amount, due_date, occurred_at }
    // - BudgetDraftCreated { family_id, budget_id, year_month, occurred_at }
    // - BudgetApproved { family_id, budget_id, approved_by, approved_at }
    // - BudgetAlertTriggered { family_id, budget_id, envelope_id, spent_percent, threshold_percent, occurred_at }
    // - GoalCreated { family_id, goal_id, name, target_amount, target_date, occurred_at }
    // - GoalContributionRecorded { family_id, goal_id, contribution_id, amount, occurred_at }
    // - ExchangeRatesRefreshed { base_currency, rates_count, fetched_at }

    // ── TODO: Sprint 6 — Notification & Device Context ──────────────────────────
    // - PushTokenRegistered { user_id, device_token, platform, registered_at }
    // - DailyDigestSent { family_id, user_id, date, sent_at }
}

impl DomainEvent {
    /// Returns the family ID associated with this event.
    /// All events in FamilyLedger belong to a family.
    pub fn family_id(&self) -> FamilyId {
        match self {
            DomainEvent::FamilyCreated { family_id, .. }
            | DomainEvent::MemberInvited { family_id, .. }
            | DomainEvent::MemberJoined { family_id, .. }
            | DomainEvent::MemberRoleChanged { family_id, .. }
            | DomainEvent::MemberRemoved { family_id, .. } => *family_id,
        }
    }

    /// Returns the timestamp when this event occurred.
    pub fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            DomainEvent::FamilyCreated { occurred_at, .. }
            | DomainEvent::MemberInvited { occurred_at, .. }
            | DomainEvent::MemberJoined { occurred_at, .. }
            | DomainEvent::MemberRoleChanged { occurred_at, .. }
            | DomainEvent::MemberRemoved { occurred_at, .. } => *occurred_at,
        }
    }
}
