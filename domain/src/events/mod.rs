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
    // ── Sprint 1: Identity & Family context ─────────────────────────────────────
    FamilyCreated {
        family_id: Uuid,
        name: String,
        home_currency: String,
        occurred_at: DateTime<Utc>,
    },
    MemberInvited {
        family_id: Uuid,
        invite_token: Uuid,
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

    // ── TODO: Sprint 2 — Payment Cards & Accounts Context ───────────────────────
    // - AccountCreated { family_id, account_id, owner_member_id, kind, currency, occurred_at }
    // - AccountUpdated { family_id, account_id, name, credit_limit, color, occurred_at }
    // - AccountDeleted { family_id, account_id, occurred_at }
    // - BalanceSnapshotComputed { family_id, account_id, year_month, closing_balance, occurred_at }
    // - AccountPeriodFrozen { family_id, account_id, year_month, frozen_by, occurred_at }

    // ── TODO: Sprint 3 — Ledger Core Context ─────────────────────────────────────
    // - TransactionRecorded { family_id, transaction_id, recorded_by, kind, amount, occurred_at }
    // - TransactionAmended { family_id, transaction_id, amendment_of_id, amount, occurred_at }
    // - TransactionDeleted { family_id, transaction_id, occurred_at }
    // - CategoryCreated { family_id, category_id, name, kind, occurred_at }
    // - ExchangeRateUpdated { base_currency, quote_currency, rate, fetched_at }

    // ── TODO: Sprint 4 — Debt Planner & Recurring Payments Context ───────────────
    // - LoanOpened { family_id, loan_id, name, loan_kind_id, principal, occurred_at }
    // - LoanPaymentRecorded { family_id, loan_id, payment_id, source_account_id, amount, principal_portion, interest_portion, remaining_balance, occurred_at }
    // - RepaymentPlanGenerated { family_id, plan_id, strategy, extra_budget, estimated_payoff, occurred_at }
    // - RecurringPaymentCreated { family_id, recurring_id, name, amount, frequency, occurred_at }
    // - RecurringPaymentDue { family_id, recurring_id, amount, due_date }
    // - RecurringPaymentProcessed { family_id, recurring_id, record_id, actual_amount, paid_at, occurred_at }

    // ── TODO: Sprint 5 — Budget & Planning Context ──────────────────────────────
    // - BudgetCreated { family_id, budget_id, year_month, occurred_at }
    // - BudgetApproved { family_id, budget_id, approved_by, approved_at }
    // - EnvelopeLimitUpdated { family_id, budget_id, envelope_id, limit_amount, occurred_at }
    // - EnvelopeAlertTriggered { family_id, budget_id, envelope_id, spent_percent, threshold_percent, occurred_at }
    // - SavingsGoalCreated { family_id, goal_id, name, target_amount, target_date, occurred_at }
    // - GoalContributionRecorded { family_id, goal_id, contribution_id, amount, occurred_at }
    // - SavingsGoalAchieved { family_id, goal_id, target_amount, achieved_at }

    // ── TODO: Sprint 6 — Notification & Device Context ──────────────────────────
    // - PushTokenRegistered { user_id, device_token, platform, registered_at }
    // - DailyDigestSent { family_id, user_id, date, sent_at }
}

impl DomainEvent {
    /// Returns the family ID associated with this event.
    /// All events in FamilyLedger belong to a family.
    pub fn family_id(&self) -> Uuid {
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
