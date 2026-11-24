//! Layer 2a PostgreSQL 16 integration tests for [`DsqlFamilyRepository`].
//!
//! Run with Docker active:
//! `cargo test -p infrastructure -- --ignored`

use anyhow::{Context, Result};
use chrono::{DateTime, SubsecRound, Utc};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};

use domain::{
    entities::{Family, FamilyParams, InviteToken},
    errors::DomainError,
    ports::{FamilyRepository, MembershipReader, TokenScope},
    value_objects::{
        CurrencyCode, DisplayName, FamilyId, FamilyName, InviteTokenId, InviteTtl, MemberId,
        Permission, Relationship, Role, UserId,
    },
};
use infrastructure::{db::migrations::run_postgres_migrations, repositories::DsqlFamilyRepository};

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn family_repository_round_trips_aggregate_with_other_role_relationship_and_invite()
-> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);

    let (family, other_uid, invite_token) = build_family_with_other_member()?;
    repo.insert_new(&family).await?;

    let loaded = repo
        .find_by_id(family.id())
        .await?
        .context("saved family should be found")?;
    assert!(loaded.invite_tokens().is_empty());
    assert_loaded_other_member(&loaded, other_uid)?;

    let inspected = repo
        .update(family.id(), TokenScope::One(invite_token), |fam| {
            Ok((fam.clone(), Vec::new()))
        })
        .await?
        .value;
    assert_loaded_family_and_invite(&inspected, &family, invite_token)?;
    assert_eq!(inspected.members().len(), 2);
    assert_eq!(inspected.invite_tokens().len(), 1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn find_by_id_returns_none_for_unknown_and_some_for_soft_deleted_records() -> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);

    assert!(repo.find_by_id(FamilyId::new_v4()).await?.is_none());

    let (deleted_family, removed_member_id) = build_soft_deleted_family_fixture()?;
    repo.insert_new(&deleted_family).await?;

    let loaded = repo
        .find_by_id(deleted_family.id())
        .await?
        .context("soft-deleted family must still reconstitute via find_by_id")?;
    assert!(loaded.is_deleted());
    assert_eq!(loaded.members().len(), 2);
    let soft_deleted_member = loaded
        .members()
        .iter()
        .find(|m| m.id() == removed_member_id)
        .context("soft-deleted member must be loaded")?;
    assert!(!soft_deleted_member.is_active());
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn list_for_user_and_membership_reader_filter_soft_deleted_and_non_members() -> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);

    let user_a = UserId::new_v4();
    let user_b = UserId::new_v4();
    let (active_family_id, removed_member_family_id) =
        seed_multi_family_fixtures(&repo, user_a, user_b).await?;

    let user_a_families = repo.list_for_user(user_a).await?;
    assert_eq!(user_a_families.len(), 1);
    assert_eq!(user_a_families[0].family_id, active_family_id);
    assert_eq!(user_a_families[0].role, Role::Owner);
    assert_eq!(user_a_families[0].display_name.as_str(), "Alice");

    let active_access = repo
        .find_active_membership(active_family_id, user_a)
        .await?
        .context("user_a is active owner")?;
    assert_eq!(active_access.role, Role::Owner);

    assert!(
        repo.find_active_membership(active_family_id, user_b)
            .await?
            .is_none()
    );
    assert!(
        repo.find_active_membership(removed_member_family_id, user_a)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn update_profile_persists_name_and_currency_while_preserving_created_at_and_members()
-> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);

    let (family, _, _) = build_family_with_other_member()?;
    let owner_uid = family.members()[0].user_id();
    let original_created_at = family.created_at();
    repo.insert_new(&family).await?;

    let new_name = FamilyName::try_new("Updated Household")?;
    let new_curr = CurrencyCode::try_new("GBP")?;
    repo.update(family.id(), TokenScope::None, move |fam| {
        fam.update_profile(owner_uid, new_name.clone(), new_curr.clone())?;
        Ok(((), Vec::new()))
    })
    .await?;

    let reloaded = repo
        .find_by_id(family.id())
        .await?
        .context("updated family should be found")?;
    assert_eq!(reloaded.name().as_str(), "Updated Household");
    assert_eq!(reloaded.home_currency().as_str(), "GBP");
    assert_eq!(reloaded.created_at(), original_created_at);
    assert_eq!(reloaded.members().len(), 2);
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn reactivated_member_round_trips_preserving_id_and_updating_role_and_permissions()
-> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);

    let (family, owner_uid, member_uid, original_member_id) = build_removed_member_fixture()?;
    repo.insert_new(&family).await?;

    let inv_tok = reactivate_member_as_other(&repo, family.id(), owner_uid, member_uid).await?;
    let reloaded = repo
        .update(family.id(), TokenScope::One(inv_tok), |fam| {
            Ok((fam.clone(), Vec::new()))
        })
        .await?
        .value;
    assert_reactivated_member(&reloaded, member_uid, original_member_id, 1)?;

    let access = repo
        .find_active_membership(family.id(), member_uid)
        .await?
        .context("reactivated member should have active membership")?;
    assert_eq!(
        (access.role, access.permissions),
        (Role::Other, vec![Permission::ViewBudget, Permission::ViewLoans])
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn role_change_and_member_removal_persist_across_reload_and_auth_queries() -> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);

    let (family, owner_uid, member_uid, member_id) = build_owner_and_member_fixture()?;
    repo.insert_new(&family).await?;

    let new_perms = vec![Permission::ViewBudget, Permission::ViewReports];
    let perms_for_op = new_perms.clone();
    repo.update(family.id(), TokenScope::None, move |fam| {
        let events = fam.change_member_role(owner_uid, member_id, Role::Other, perms_for_op.clone())?;
        Ok(((), events))
    })
    .await?;
    let access = repo
        .find_active_membership(family.id(), member_uid)
        .await?
        .context("updated member should remain active")?;
    assert_eq!((access.role, access.permissions), (Role::Other, new_perms));

    repo.update(family.id(), TokenScope::None, move |fam| {
        let events = fam.remove_member(owner_uid, member_id)?;
        Ok(((), events))
    })
    .await?;
    assert!(repo.find_active_membership(family.id(), member_uid).await?.is_none());
    assert!(repo.list_for_user(member_uid).await?.is_empty());
    assert_soft_deleted_member_persisted(&repo, family.id(), member_id).await
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn concurrent_accept_same_invite_token_allows_exactly_one_winner() -> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);
    let (family, _, token) = build_family_with_unused_invite()?;
    repo.insert_new(&family).await?;

    let (u1, u2) = (UserId::new_v4(), UserId::new_v4());
    let (r1, r2) = tokio::join!(
        accept_invite_via_repo(&repo, family.id(), token, u1, "User One"),
        accept_invite_via_repo(&repo, family.id(), token, u2, "User Two"),
    );
    assert_one_ok_and_one_err(r1, r2, |err| matches!(err, DomainError::TokenAlreadyUsed))?;

    let reloaded = repo.find_by_id(family.id()).await?.context("family")?;
    assert_eq!(reloaded.active_members().count(), 2);
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn concurrent_join_same_user_with_two_tokens_maps_duplicate_to_member_already_exists()
-> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);
    let (family, tok1, tok2) = build_family_with_two_unused_invites()?;
    repo.insert_new(&family).await?;

    let same_user = UserId::new_v4();
    let (r1, r2) = tokio::join!(
        accept_invite_via_repo(&repo, family.id(), tok1, same_user, "Join A"),
        accept_invite_via_repo(&repo, family.id(), tok2, same_user, "Join B"),
    );
    assert_one_ok_and_one_err(r1, r2, |err| {
        matches!(err, DomainError::MemberAlreadyExists { .. })
    })?;

    let reloaded = repo.find_by_id(family.id()).await?.context("family")?;
    assert_eq!(reloaded.active_members().count(), 2);
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn concurrent_profile_update_and_member_removal_never_resurrects_removed_member()
-> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);
    let (family, owner_uid, _, member_id) = build_owner_and_member_fixture()?;
    repo.insert_new(&family).await?;

    let new_name = FamilyName::try_new("Concurrent Name")?;
    let new_curr = CurrencyCode::try_new("EUR")?;
    let (r_prof, r_rem) = tokio::join!(
        repo.update(family.id(), TokenScope::None, move |fam| {
            fam.update_profile(owner_uid, new_name.clone(), new_curr.clone())?;
            Ok(((), Vec::new()))
        }),
        repo.update(family.id(), TokenScope::None, move |fam| {
            let events = fam.remove_member(owner_uid, member_id)?;
            Ok(((), events))
        }),
    );
    r_prof?;
    r_rem?;

    let reloaded = repo.find_by_id(family.id()).await?.context("family")?;
    assert_eq!(reloaded.name().as_str(), "Concurrent Name");
    assert_eq!(reloaded.home_currency().as_str(), "EUR");
    assert!(reloaded.active_members().all(|m| m.id() != member_id));
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn update_rolls_back_and_persists_nothing_when_closure_returns_err() -> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);
    let (family, owner_uid, _, member_id) = build_owner_and_member_fixture()?;
    repo.insert_new(&family).await?;

    let res = repo
        .update(family.id(), TokenScope::None, move |fam| {
            fam.remove_member(owner_uid, member_id)?;
            Err::<((), Vec<domain::events::DomainEvent>), DomainError>(
                DomainError::InvariantViolation {
                    message: "force rollback".to_owned(),
                },
            )
        })
        .await;
    assert!(matches!(res, Err(DomainError::InvariantViolation { .. })));

    let reloaded = repo.find_by_id(family.id()).await?.context("family")?;
    assert_eq!(reloaded.active_members().count(), 2);
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn noop_role_change_in_update_succeeds_with_zero_events_and_preserves_state() -> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);
    let (family, owner_uid, _, member_id) = build_owner_and_member_fixture()?;
    repo.insert_new(&family).await?;

    let updated = repo
        .update(family.id(), TokenScope::None, move |fam| {
            let events = fam.change_member_role(owner_uid, member_id, Role::Member, vec![])?;
            Ok(((), events))
        })
        .await?;
    assert!(updated.events.is_empty());

    let reloaded = repo.find_by_id(family.id()).await?.context("family")?;
    assert_eq!(reloaded.active_members().count(), 2);
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn family_with_600_historical_invite_tokens_remains_mutable() -> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool.clone());
    let (family, owner_uid, _, member_id) = build_owner_and_member_fixture()?;
    let owner_member_id = family.members()[0].id();
    repo.insert_new(&family).await?;
    seed_historical_used_tokens(&pool, family.id(), owner_member_id, 600).await?;

    let new_name = FamilyName::try_new("Long-Lived Household")?;
    let new_curr = CurrencyCode::try_new("GBP")?;
    repo.update(family.id(), TokenScope::None, move |fam| {
        fam.update_profile(owner_uid, new_name.clone(), new_curr.clone())?;
        Ok(((), Vec::new()))
    })
    .await?;
    repo.update(family.id(), TokenScope::None, move |fam| {
        let (inv, events) =
            fam.create_invite(owner_uid, Role::Child, vec![], None, InviteTtl::from_hours(24)?)?;
        Ok((inv.token(), events))
    })
    .await?;
    repo.update(family.id(), TokenScope::None, move |fam| {
        let events = fam.remove_member(owner_uid, member_id)?;
        Ok(((), events))
    })
    .await?;

    let reloaded = repo.find_by_id(family.id()).await?.context("family")?;
    assert_eq!(reloaded.name().as_str(), "Long-Lived Household");
    assert_eq!(reloaded.active_members().count(), 1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker (run: cargo test -p infrastructure -- --ignored)"]
async fn token_scope_rejects_valid_token_belonging_to_another_family() -> Result<()> {
    let (_container, pool) = start_migrated_postgres().await?;
    let repo = DsqlFamilyRepository::new(pool);
    let (fam_a, _, token_a) = build_family_with_unused_invite()?;
    let (fam_b, _, _, _) = build_owner_and_member_fixture()?;
    repo.insert_new(&fam_a).await?;
    repo.insert_new(&fam_b).await?;

    let err = accept_invite_via_repo(&repo, fam_b.id(), token_a, UserId::new_v4(), "Intruder")
        .await
        .unwrap_err();
    assert!(matches!(err, DomainError::InviteTokenNotFound));

    let still_valid = repo
        .update(fam_a.id(), TokenScope::One(token_a), |fam| {
            Ok((fam.invite_tokens()[0].is_used(), Vec::new()))
        })
        .await?;
    assert!(!still_valid.value);
    Ok(())
}

async fn start_migrated_postgres() -> Result<(ContainerAsync<Postgres>, PgPool)> {
    let container = Postgres::default().with_tag("16-alpine").start().await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await?;
    run_postgres_migrations(&pool).await?;
    Ok((container, pool))
}

fn build_family_with_other_member() -> Result<(Family, UserId, InviteTokenId)> {
    let now = Utc::now().trunc_subsecs(6);
    let owner_uid = UserId::new_v4();
    let other_uid = UserId::new_v4();
    let (mut family, _) = Family::create_at(
        FamilyName::try_new("Smith Household")?,
        CurrencyCode::try_new("EUR")?,
        owner_uid,
        DisplayName::try_new("Alice")?,
        now,
    )?;

    let (invite, _) = family.create_invite_at(
        owner_uid,
        Role::Other,
        vec![Permission::ViewBudget, Permission::ViewReports],
        Some(Relationship::try_new("Grandma")?),
        InviteTtl::from_hours(48)?,
        now,
    )?;
    family.accept_invite_at(
        invite.token(),
        other_uid,
        DisplayName::try_new("Nana")?,
        now,
    )?;
    Ok((family, other_uid, invite.token()))
}

fn assert_loaded_family_and_invite(
    loaded: &Family,
    expected: &Family,
    invite_token: InviteTokenId,
) -> Result<()> {
    assert_eq!(loaded.id(), expected.id());
    assert_eq!(loaded.name().as_str(), "Smith Household");
    assert_eq!(loaded.home_currency().as_str(), "EUR");
    assert_eq!(loaded.created_at(), expected.created_at());
    assert_eq!(loaded.members().len(), 2);
    assert_eq!(loaded.invite_tokens().len(), 1);

    let loaded_invite = &loaded.invite_tokens()[0];
    assert_eq!(loaded_invite.token(), invite_token);
    assert!(loaded_invite.is_used());
    assert_eq!(loaded_invite.role(), Role::Other);
    assert_eq!(
        loaded_invite.permissions(),
        &[Permission::ViewBudget, Permission::ViewReports]
    );
    assert_eq!(
        loaded_invite.relationship().map(Relationship::as_str),
        Some("Grandma")
    );
    Ok(())
}

fn assert_loaded_other_member(loaded: &Family, other_uid: UserId) -> Result<()> {
    let other_member = loaded
        .members()
        .iter()
        .find(|m| m.user_id() == other_uid)
        .context("other member should exist")?;
    assert_eq!(other_member.display_name().as_str(), "Nana");
    assert_eq!(other_member.role(), Role::Other);
    assert_eq!(
        other_member.permissions(),
        &[Permission::ViewBudget, Permission::ViewReports]
    );
    assert_eq!(
        other_member.relationship().map(Relationship::as_str),
        Some("Grandma")
    );
    Ok(())
}

fn build_soft_deleted_family_fixture() -> Result<(Family, MemberId)> {
    let now = Utc::now().trunc_subsecs(6);
    let owner_uid = UserId::new_v4();
    let member_uid = UserId::new_v4();
    let (mut family, _) = Family::create_at(
        FamilyName::try_new("Deleted Test")?,
        CurrencyCode::try_new("USD")?,
        owner_uid,
        DisplayName::try_new("Owner")?,
        now,
    )?;
    let (invite, _) = family.create_invite_at(
        owner_uid,
        Role::Member,
        vec![],
        None,
        InviteTtl::from_hours(24)?,
        now,
    )?;
    let (joined, _) = family.accept_invite_at(
        invite.token(),
        member_uid,
        DisplayName::try_new("Member")?,
        now,
    )?;
    family.remove_member_at(owner_uid, joined.id(), now)?;
    Ok((mark_family_soft_deleted(&family, now), joined.id()))
}

async fn seed_multi_family_fixtures(
    repo: &DsqlFamilyRepository,
    user_a: UserId,
    user_b: UserId,
) -> Result<(FamilyId, FamilyId)> {
    let now = Utc::now().trunc_subsecs(6);
    let (active_family, _) = Family::create_at(
        FamilyName::try_new("Active Home")?,
        CurrencyCode::try_new("USD")?,
        user_a,
        DisplayName::try_new("Alice")?,
        now,
    )?;
    repo.insert_new(&active_family).await?;

    let removed_id = seed_removed_member_family(repo, user_b, user_a, now).await?;

    let (deleted_family, _) = Family::create_at(
        FamilyName::try_new("Archived Home")?,
        CurrencyCode::try_new("EUR")?,
        user_a,
        DisplayName::try_new("Alice Old")?,
        now,
    )?;
    repo.insert_new(&mark_family_soft_deleted(&deleted_family, now))
        .await?;

    Ok((active_family.id(), removed_id))
}

async fn seed_removed_member_family(
    repo: &DsqlFamilyRepository,
    owner_uid: UserId,
    removed_uid: UserId,
    now: DateTime<Utc>,
) -> Result<FamilyId> {
    let (mut family, _) = Family::create_at(
        FamilyName::try_new("Removed Member Home")?,
        CurrencyCode::try_new("GBP")?,
        owner_uid,
        DisplayName::try_new("Bob")?,
        now,
    )?;
    let (invite, _) = family.create_invite_at(
        owner_uid,
        Role::Child,
        vec![],
        None,
        InviteTtl::from_hours(24)?,
        now,
    )?;
    let (joined, _) = family.accept_invite_at(
        invite.token(),
        removed_uid,
        DisplayName::try_new("Alice Kid")?,
        now,
    )?;
    family.remove_member_at(owner_uid, joined.id(), now)?;
    repo.insert_new(&family).await?;
    Ok(family.id())
}

fn mark_family_soft_deleted(family: &Family, deleted_at: DateTime<Utc>) -> Family {
    Family::reconstitute(FamilyParams {
        id: family.id(),
        name: family.name().clone(),
        home_currency: family.home_currency().clone(),
        members: family.members().to_vec(),
        invite_tokens: family.invite_tokens().to_vec(),
        created_at: family.created_at(),
        deleted_at: Some(deleted_at),
    })
}

fn build_removed_member_fixture() -> Result<(Family, UserId, UserId, MemberId)> {
    let now = Utc::now().trunc_subsecs(6);
    let owner_uid = UserId::new_v4();
    let member_uid = UserId::new_v4();
    let (mut family, _) = Family::create_at(
        FamilyName::try_new("Reactivation Home")?,
        CurrencyCode::try_new("USD")?,
        owner_uid,
        DisplayName::try_new("Owner")?,
        now,
    )?;
    let (inv, _) = family.create_invite_at(
        owner_uid,
        Role::Member,
        vec![],
        None,
        InviteTtl::from_hours(24)?,
        now,
    )?;
    let (joined, _) = family.accept_invite_at(
        inv.token(),
        member_uid,
        DisplayName::try_new("Initial")?,
        now,
    )?;
    family.remove_member_at(owner_uid, joined.id(), now)?;
    Ok((family, owner_uid, member_uid, joined.id()))
}

fn assert_reactivated_member(
    reloaded: &Family,
    member_uid: UserId,
    original_member_id: MemberId,
    expected_loaded_tokens: usize,
) -> Result<()> {
    let member = reloaded
        .members()
        .iter()
        .find(|m| m.user_id() == member_uid)
        .context("reactivated member should exist")?;
    assert_eq!(member.id(), original_member_id);
    assert!(member.is_active());
    assert_eq!(member.role(), Role::Other);
    assert_eq!(member.display_name().as_str(), "Reactivated");
    assert_eq!(
        member.permissions(),
        &[Permission::ViewBudget, Permission::ViewLoans]
    );
    assert_eq!(
        member.relationship().map(Relationship::as_str),
        Some("Aunt")
    );
    assert_eq!(reloaded.invite_tokens().len(), expected_loaded_tokens);
    assert!(reloaded.invite_tokens().iter().all(InviteToken::is_used));
    Ok(())
}

async fn reactivate_member_as_other(
    repo: &DsqlFamilyRepository,
    family_id: FamilyId,
    owner_uid: UserId,
    member_uid: UserId,
) -> Result<InviteTokenId> {
    let inv_tok = repo
        .update(family_id, TokenScope::None, move |fam| {
            let (inv, events) = fam.create_invite(
                owner_uid,
                Role::Other,
                vec![Permission::ViewBudget, Permission::ViewLoans],
                Some(Relationship::try_new("Aunt")?),
                InviteTtl::from_hours(48)?,
            )?;
            Ok((inv.token(), events))
        })
        .await?
        .value;
    accept_invite_via_repo(repo, family_id, inv_tok, member_uid, "Reactivated").await?;
    Ok(inv_tok)
}

fn build_owner_and_member_fixture() -> Result<(Family, UserId, UserId, MemberId)> {
    let now = Utc::now().trunc_subsecs(6);
    let owner_uid = UserId::new_v4();
    let member_uid = UserId::new_v4();
    let (mut family, _) = Family::create_at(
        FamilyName::try_new("Governance Home")?,
        CurrencyCode::try_new("USD")?,
        owner_uid,
        DisplayName::try_new("Owner")?,
        now,
    )?;
    let (inv, _) = family.create_invite_at(
        owner_uid,
        Role::Member,
        vec![],
        None,
        InviteTtl::from_hours(24)?,
        now,
    )?;
    let (joined, _) = family.accept_invite_at(
        inv.token(),
        member_uid,
        DisplayName::try_new("Member")?,
        now,
    )?;
    Ok((family, owner_uid, member_uid, joined.id()))
}

async fn assert_soft_deleted_member_persisted(
    repo: &DsqlFamilyRepository,
    family_id: FamilyId,
    member_id: MemberId,
) -> Result<()> {
    let reloaded = repo
        .find_by_id(family_id)
        .await?
        .context("family should remain persisted")?;
    let removed = reloaded
        .members()
        .iter()
        .find(|m| m.id() == member_id)
        .context("soft-deleted member row must be retained")?;
    assert!(!removed.is_active());
    assert!(removed.deleted_at().is_some());
    assert_eq!(removed.role(), Role::Other);
    assert_eq!(
        removed.permissions(),
        &[Permission::ViewBudget, Permission::ViewReports]
    );
    Ok(())
}

fn build_family_with_unused_invite() -> Result<(Family, UserId, InviteTokenId)> {
    let now = Utc::now().trunc_subsecs(6);
    let owner_uid = UserId::new_v4();
    let (mut family, _) = Family::create_at(
        FamilyName::try_new("Concurrent Home")?,
        CurrencyCode::try_new("USD")?,
        owner_uid,
        DisplayName::try_new("Owner")?,
        now,
    )?;
    let (inv, _) =
        family.create_invite_at(owner_uid, Role::Member, vec![], None, InviteTtl::from_hours(24)?, now)?;
    Ok((family, owner_uid, inv.token()))
}

fn build_family_with_two_unused_invites() -> Result<(Family, InviteTokenId, InviteTokenId)> {
    let (mut family, owner_uid, tok1) = build_family_with_unused_invite()?;
    let (inv2, _) =
        family.create_invite(owner_uid, Role::Child, vec![], None, InviteTtl::from_hours(24)?)?;
    Ok((family, tok1, inv2.token()))
}

async fn accept_invite_via_repo(
    repo: &DsqlFamilyRepository,
    family_id: FamilyId,
    token: InviteTokenId,
    user_id: UserId,
    name: &str,
) -> Result<(), DomainError> {
    let display_name = DisplayName::try_new(name)?;
    repo.update(family_id, TokenScope::One(token), move |fam| {
        let (_, events) = fam.accept_invite(token, user_id, display_name.clone())?;
        Ok(((), events))
    })
    .await
    .map(|_| ())
}

fn assert_one_ok_and_one_err<F>(
    r1: Result<(), DomainError>,
    r2: Result<(), DomainError>,
    pred: F,
) -> Result<()>
where
    F: Fn(&DomainError) -> bool,
{
    match (r1, r2) {
        (Ok(()), Err(e)) | (Err(e), Ok(())) if pred(&e) => Ok(()),
        (a, b) => anyhow::bail!("expected 1 Ok and 1 matching Err, got ({a:?}, {b:?})"),
    }
}

async fn seed_historical_used_tokens(
    pool: &PgPool,
    family_id: FamilyId,
    created_by: MemberId,
    count: usize,
) -> Result<()> {
    let now = Utc::now().trunc_subsecs(6);
    for _ in 0..count {
        sqlx::query(
            "INSERT INTO family_invite_tokens \
             (token, family_id, role, permissions, relationship, created_by, created_at, expires_at, used) \
             VALUES ($1, $2, 'member', '[]'::jsonb, NULL, $3, $4, $5, true)",
        )
        .bind(InviteTokenId::new_v4().into_inner())
        .bind(family_id.into_inner())
        .bind(created_by.into_inner())
        .bind(now)
        .bind(now + chrono::Duration::hours(24))
        .execute(pool)
        .await?;
    }
    Ok(())
}
