//! Blind conformance tests for `backend/specs/domain/value-objects.md` (`VO-01`..`VO-10`).

use chrono::Duration;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use domain::errors::DomainError;
use domain::value_objects::currency_code::CurrencyCodeError;
use domain::value_objects::display_name::DisplayNameError;
use domain::value_objects::email::EmailError;
use domain::value_objects::family_name::FamilyNameError;
use domain::value_objects::invite_ttl::InviteTtlError;
use domain::value_objects::relationship::RelationshipError;
use domain::value_objects::{
    CurrencyCode, DisplayName, Email, FamilyName, InviteTtl, MAX_INVITE_TTL_SECONDS, Money,
    Relationship,
};

#[test]
fn vo_01_money_construction_rejects_negative_and_accepts_non_negative() {
    let usd = CurrencyCode::try_new("USD").unwrap();

    let zero = Money::new(dec!(0.00), usd.clone()).unwrap();
    assert!(zero.is_zero());
    assert_eq!(zero.amount(), dec!(0.00));

    let positive = Money::new(dec!(0.01), usd.clone()).unwrap();
    assert!(!positive.is_zero());
    assert_eq!(positive.amount(), dec!(0.01));

    let err = Money::new(dec!(-0.01), usd).unwrap_err();
    assert_eq!(
        err,
        DomainError::NegativeAmount {
            amount: dec!(-0.01)
        }
    );
}

#[test]
fn vo_02_money_addition_requires_matching_currency_and_detects_overflow() {
    let usd = CurrencyCode::try_new("USD").unwrap();
    let eur = CurrencyCode::try_new("EUR").unwrap();

    let sum = Money::new(dec!(10.50), usd.clone())
        .unwrap()
        .add(&Money::new(dec!(4.50), usd.clone()).unwrap())
        .unwrap();
    assert_eq!(sum.amount(), dec!(15.00));
    assert_eq!(sum.currency(), &usd);

    let mismatch = Money::zero(usd.clone())
        .add(&Money::zero(eur.clone()))
        .unwrap_err();
    assert_eq!(
        mismatch,
        DomainError::CurrencyMismatch {
            expected: usd.clone(),
            actual: eur
        }
    );

    let overflow = Money::new(Decimal::MAX, usd.clone())
        .unwrap()
        .add(&Money::new(dec!(1), usd).unwrap())
        .unwrap_err();
    assert_eq!(
        overflow,
        DomainError::ArithmeticOverflow {
            operation: "money addition"
        }
    );
}

#[test]
fn vo_03_money_subtraction_requires_matching_currency_and_non_negative_result() {
    let usd = CurrencyCode::try_new("USD").unwrap();
    let mdl = CurrencyCode::try_new("MDL").unwrap();

    let diff = Money::new(dec!(50.00), usd.clone())
        .unwrap()
        .sub(&Money::new(dec!(20.00), usd.clone()).unwrap())
        .unwrap();
    assert_eq!(diff.amount(), dec!(30.00));

    let zero_diff = Money::new(dec!(50.00), usd.clone())
        .unwrap()
        .sub(&Money::new(dec!(50.00), usd.clone()).unwrap())
        .unwrap();
    assert!(zero_diff.is_zero());

    let negative = Money::new(dec!(5.00), usd.clone())
        .unwrap()
        .sub(&Money::new(dec!(10.00), usd.clone()).unwrap())
        .unwrap_err();
    assert_eq!(
        negative,
        DomainError::NegativeAmount {
            amount: dec!(-5.00)
        }
    );

    let mismatch = Money::new(dec!(50.00), usd.clone())
        .unwrap()
        .sub(&Money::new(dec!(10.00), mdl.clone()).unwrap())
        .unwrap_err();
    assert_eq!(
        mismatch,
        DomainError::CurrencyMismatch {
            expected: usd,
            actual: mdl
        }
    );
}

#[test]
fn vo_04_currency_code_enforces_three_trimmed_uppercase_ascii_letters() {
    let usd = CurrencyCode::try_new("USD").unwrap();
    assert_eq!(usd.as_str(), "USD");

    let eur = CurrencyCode::try_new("  EUR  ").unwrap();
    assert_eq!(eur.as_str(), "EUR");

    for invalid in ["usd", "US", "USDD", "", "   ", "U$D"] {
        let err = CurrencyCode::try_new(invalid).unwrap_err();
        assert!(matches!(err, CurrencyCodeError::PredicateViolated));
        assert!(matches!(
            DomainError::from(err),
            DomainError::InvalidCurrencyCode { .. }
        ));
    }
}

#[test]
fn vo_05_family_name_trims_and_enforces_1_to_200_unicode_chars() {
    let trimmed = FamilyName::try_new("  Smith Family  ").unwrap();
    assert_eq!(trimmed.as_str(), "Smith Family");

    for empty in ["", "   "] {
        let err = FamilyName::try_new(empty).unwrap_err();
        assert!(matches!(err, FamilyNameError::NotEmptyViolated));
        assert!(matches!(
            DomainError::from(err),
            DomainError::InvalidFamilyName { .. }
        ));
    }

    let max_ascii = "a".repeat(200);
    assert_eq!(FamilyName::try_new(&max_ascii).unwrap().as_str(), max_ascii);

    let max_unicode = "é".repeat(200);
    assert_eq!(
        FamilyName::try_new(&max_unicode).unwrap().as_str(),
        max_unicode
    );

    let too_long = "a".repeat(201);
    let err = FamilyName::try_new(&too_long).unwrap_err();
    assert!(matches!(err, FamilyNameError::LenCharMaxViolated));
    assert!(matches!(
        DomainError::from(err),
        DomainError::InvalidFamilyName { .. }
    ));
}

#[test]
fn vo_06_money_multiply_and_deserialization_preserve_invariants() {
    let usd = CurrencyCode::try_new("USD").unwrap();
    let base = Money::new(dec!(1000.00), usd.clone()).unwrap();
    let scaled = base.multiply(dec!(1.05)).unwrap();
    assert_eq!(scaled.amount(), dec!(1050.00));
    assert_eq!(scaled.currency(), &usd);

    let neg_factor = Money::zero(usd.clone()).multiply(dec!(-1)).unwrap_err();
    assert_eq!(neg_factor, DomainError::NegativeAmount { amount: dec!(-1) });

    let overflow = Money::new(Decimal::MAX, usd)
        .unwrap()
        .multiply(dec!(2))
        .unwrap_err();
    assert_eq!(
        overflow,
        DomainError::ArithmeticOverflow {
            operation: "money multiplication"
        }
    );

    assert!(serde_json::from_str::<Money>(r#"{"amount":"-1.00","currency":"USD"}"#).is_err());
    assert!(serde_json::from_str::<Money>(r#"{"amount":"1.00","currency":"usd"}"#).is_err());
    let valid: Money = serde_json::from_str(r#"{"amount":"1.00","currency":"USD"}"#).unwrap();
    assert_eq!(valid.amount(), dec!(1.00));
}

#[test]
fn vo_07_display_name_trims_and_enforces_1_to_100_unicode_chars() {
    let trimmed = DisplayName::try_new("  Alice  ").unwrap();
    assert_eq!(trimmed.as_str(), "Alice");

    for empty in ["", "   "] {
        let err = DisplayName::try_new(empty).unwrap_err();
        assert!(matches!(err, DisplayNameError::NotEmptyViolated));
        assert!(matches!(
            DomainError::from(err),
            DomainError::InvalidDisplayName { .. }
        ));
    }

    let max_chars = "a".repeat(100);
    assert_eq!(
        DisplayName::try_new(&max_chars).unwrap().as_str(),
        max_chars
    );

    let over_max = "a".repeat(101);
    let err = DisplayName::try_new(&over_max).unwrap_err();
    assert!(matches!(err, DisplayNameError::LenCharMaxViolated));
    assert!(matches!(
        DomainError::from(err),
        DomainError::InvalidDisplayName { .. }
    ));
}

#[test]
fn vo_08_email_trims_lowercases_and_enforces_bounded_rfc_syntax() {
    let normalized = Email::try_new("  Alice@Example.COM  ").unwrap();
    assert_eq!(normalized.as_str(), "alice@example.com");

    for empty in ["", "   "] {
        let err = Email::try_new(empty).unwrap_err();
        assert!(matches!(err, EmailError::NotEmptyViolated));
        assert!(matches!(
            DomainError::from(err),
            DomainError::InvalidEmail { .. }
        ));
    }

    let max_254 = format!(
        "{}@{}.{}.{}.com",
        "a".repeat(64),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(57)
    );
    assert_eq!(max_254.len(), 254);
    assert!(Email::try_new(&max_254).is_ok());

    let over_255 = format!(
        "{}@{}.{}.{}.com",
        "a".repeat(64),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(58)
    );
    assert_eq!(over_255.len(), 255);

    for invalid in [
        "alice@localhost",
        "alice@[127.0.0.1]",
        "Alice <alice@example.com>",
        "\"quoted\"@example.com",
        "alice@example.c",
        "alice@example.123",
        over_255.as_str(),
    ] {
        let err = Email::try_new(invalid).unwrap_err();
        assert!(matches!(err, EmailError::PredicateViolated));
        assert!(matches!(
            DomainError::from(err),
            DomainError::InvalidEmail { .. }
        ));
    }
}

#[test]
fn vo_09_relationship_trims_and_enforces_1_to_50_unicode_chars() {
    let trimmed = Relationship::try_new("  Grandma  ").unwrap();
    assert_eq!(trimmed.as_str(), "Grandma");

    for empty in ["", "   "] {
        let err = Relationship::try_new(empty).unwrap_err();
        assert!(matches!(err, RelationshipError::NotEmptyViolated));
        assert!(matches!(
            DomainError::from(err),
            DomainError::InvalidRelationship { .. }
        ));
    }

    let max_chars = "a".repeat(50);
    assert_eq!(
        Relationship::try_new(&max_chars).unwrap().as_str(),
        max_chars
    );

    let over_max = "a".repeat(51);
    let err = Relationship::try_new(&over_max).unwrap_err();
    assert!(matches!(err, RelationshipError::LenCharMaxViolated));
    assert!(matches!(
        DomainError::from(err),
        DomainError::InvalidRelationship { .. }
    ));
}

#[test]
fn vo_10_invite_ttl_enforces_1_to_30_days_whole_seconds() {
    assert_eq!(MAX_INVITE_TTL_SECONDS, 2_592_000);
    assert_eq!(InviteTtl::try_new(1).unwrap().into_inner(), 1);
    assert_eq!(InviteTtl::from_days(30).unwrap().into_inner(), 2_592_000);
    assert_eq!(
        InviteTtl::try_from_duration(Duration::seconds(60))
            .unwrap()
            .into_inner(),
        60
    );

    for non_positive in [0, -1] {
        let err = InviteTtl::try_new(non_positive).unwrap_err();
        assert!(matches!(err, InviteTtlError::GreaterViolated));
        assert!(matches!(
            DomainError::from(err),
            DomainError::InvalidInviteTtl { .. }
        ));
    }

    let over_max = InviteTtl::try_new(2_592_001).unwrap_err();
    assert!(matches!(over_max, InviteTtlError::LessOrEqualViolated));
    assert!(matches!(
        InviteTtl::from_days(31),
        Err(DomainError::InvalidInviteTtl { .. })
    ));
    assert!(matches!(
        InviteTtl::from_hours(i64::MAX),
        Err(DomainError::InvalidInviteTtl { .. })
    ));
    assert!(matches!(
        InviteTtl::from_days(i64::MAX),
        Err(DomainError::InvalidInviteTtl { .. })
    ));
    assert!(matches!(
        InviteTtl::try_from_duration(Duration::zero()),
        Err(DomainError::InvalidInviteTtl { .. })
    ));
    assert!(matches!(
        InviteTtl::try_from_duration(Duration::milliseconds(1500)),
        Err(DomainError::InvalidInviteTtl { .. })
    ));
    assert!(matches!(
        InviteTtl::try_from_duration(Duration::seconds(2_592_001)),
        Err(DomainError::InvalidInviteTtl { .. })
    ));
}
