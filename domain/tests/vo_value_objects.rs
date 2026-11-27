//! Blind conformance tests for `backend/specs/domain/value-objects.md` (`VO-01`..`VO-18`).

use chrono::{Duration, TimeZone, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use domain::errors::DomainError;
use domain::value_objects::account_fields::{AccountNameError, BankNameError, LastFourError};
use domain::value_objects::currency_code::CurrencyCodeError;
use domain::value_objects::email::EmailError;
use domain::value_objects::family_name::FamilyNameError;
use domain::value_objects::hex_color::HexColorError;
use domain::value_objects::invite_ttl::InviteTtlError;
use domain::value_objects::ledger_fields::{
    CategoryIconError, CategoryNameError, TagError, TransactionDescriptionError,
    MAX_TAGS_PER_TRANSACTION, validate_tags,
};
use domain::value_objects::member_display_name::MemberDisplayNameError;
use domain::value_objects::relationship::RelationshipError;
use domain::value_objects::{
    AccountName, BankName, CategoryIcon, CategoryName, CurrencyCode, Email, FamilyName, HexColor,
    InviteTokenHash, InviteTokenId, InviteTtl, LastFour, MAX_INVITE_TTL_SECONDS,
    MemberDisplayName, Money, PositiveMoney, Relationship, SignedMoney, Tag,
    TransactionDescription, YearMonth,
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
    let trimmed = MemberDisplayName::try_new("  Alice  ").unwrap();
    assert_eq!(trimmed.as_str(), "Alice");

    for empty in ["", "   "] {
        let err = MemberDisplayName::try_new(empty).unwrap_err();
        assert!(matches!(err, MemberDisplayNameError::NotEmptyViolated));
        assert!(matches!(
            DomainError::from(err),
            DomainError::InvalidDisplayName { .. }
        ));
    }

    let max_chars = "a".repeat(100);
    assert_eq!(
        MemberDisplayName::try_new(&max_chars).unwrap().as_str(),
        max_chars
    );

    let over_max = "a".repeat(101);
    let err = MemberDisplayName::try_new(&over_max).unwrap_err();
    assert!(matches!(err, MemberDisplayNameError::LenCharMaxViolated));
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

#[test]
fn vo_11_display_text_vos_reject_control_and_bidi_characters() {
    let ok = "  Família da Silva  ";
    assert_eq!(FamilyName::try_new(ok).unwrap().as_str(), "Família da Silva");
    assert_eq!(MemberDisplayName::try_new(ok).unwrap().as_str(), "Família da Silva");
    assert_eq!(Relationship::try_new(ok).unwrap().as_str(), "Família da Silva");

    for bad in ["Alice\0", "Alice\nBob", "Alice\x1b[31m", "Alice\u{202E}boB", "Alice\u{2066}x"] {
        assert!(matches!(FamilyName::try_new(bad), Err(FamilyNameError::PredicateViolated)));
        assert!(matches!(
            MemberDisplayName::try_new(bad),
            Err(MemberDisplayNameError::PredicateViolated)
        ));
        assert!(matches!(Relationship::try_new(bad), Err(RelationshipError::PredicateViolated)));
    }
}

#[test]
fn vo_12_invite_token_hash_is_64_lowercase_hex_and_matches_token() {
    let token = InviteTokenId::new_v4();
    let hash = InviteTokenHash::from_token_id(token);
    assert_eq!(hash.as_str().len(), 64);
    assert!(hash.matches_token(token));
    assert!(!hash.matches_token(InviteTokenId::new_v4()));
    assert_eq!(hash, InviteTokenHash::try_new(format!("  {}  ", hash.as_str())).unwrap());

    for bad in ["A".repeat(64), "a".repeat(63), "g".repeat(64)] {
        assert!(matches!(
            InviteTokenHash::try_new(&bad),
            Err(DomainError::InvariantViolation { .. })
        ));
    }
}

#[test]
fn vo_13_signed_money_supports_negative_zero_positive_with_currency_checks() {
    let usd = CurrencyCode::try_new("USD").unwrap();
    let eur = CurrencyCode::try_new("EUR").unwrap();
    let neg = SignedMoney::new(dec!(-120.50), usd.clone());
    let pos = SignedMoney::new(dec!(20.50), usd.clone());
    let zero = SignedMoney::zero(usd.clone());
    assert!(neg.is_negative() && pos.is_positive() && zero.is_zero());

    assert_eq!(neg.add(&pos).unwrap(), SignedMoney::new(dec!(-100.00), usd.clone()));
    assert_eq!(
        SignedMoney::new(dec!(10.00), usd.clone())
            .sub(&SignedMoney::new(dec!(25.00), usd.clone()))
            .unwrap(),
        SignedMoney::new(dec!(-15.00), usd.clone())
    );
    assert_eq!(neg.negate().unwrap(), SignedMoney::new(dec!(120.50), usd.clone()));
    assert!(matches!(
        SignedMoney::new(dec!(-10.00), usd).add(&SignedMoney::new(dec!(10.00), eur)),
        Err(DomainError::CurrencyMismatch { .. })
    ));
    let max_usd = SignedMoney::new(Decimal::MAX, CurrencyCode::try_new("USD").unwrap());
    let one_usd = SignedMoney::new(dec!(1), CurrencyCode::try_new("USD").unwrap());
    assert!(matches!(max_usd.add(&one_usd), Err(DomainError::ArithmeticOverflow { .. })));
    assert!(matches!(
        SignedMoney::new(Decimal::MIN, CurrencyCode::try_new("USD").unwrap()).sub(&one_usd),
        Err(DomainError::ArithmeticOverflow { .. })
    ));
}

#[test]
fn vo_14_positive_money_requires_strictly_positive_amounts() {
    let usd = CurrencyCode::try_new("USD").unwrap();
    let min_pos = PositiveMoney::new(dec!(0.0001), usd.clone()).unwrap();
    assert_eq!(min_pos.as_money(), Money::new(dec!(0.0001), usd.clone()).unwrap());
    let into_money: Money = min_pos.into();
    assert_eq!(into_money.amount(), dec!(0.0001));

    for non_pos in [dec!(0.00), dec!(-0.01)] {
        assert_eq!(
            PositiveMoney::new(non_pos, usd.clone()),
            Err(DomainError::NonPositiveAmount { amount: non_pos })
        );
    }
    let bad_json = r#"{"amount":"0.00","currency":"USD"}"#;
    assert!(serde_json::from_str::<PositiveMoney>(bad_json).is_err());
}

#[test]
fn vo_15_account_and_bank_names_enforce_bounds_and_hygiene() {
    assert_eq!(AccountName::try_new(" a ").unwrap().as_str(), "a");
    assert_eq!(AccountName::try_new("a".repeat(200)).unwrap().as_str().len(), 200);
    assert!(matches!(AccountName::try_new("   "), Err(AccountNameError::NotEmptyViolated)));
    assert!(matches!(AccountName::try_new("a".repeat(201)), Err(AccountNameError::LenCharMaxViolated)));
    assert!(matches!(AccountName::try_new("Card\0"), Err(AccountNameError::PredicateViolated)));

    assert_eq!(BankName::try_new(" b ").unwrap().as_str(), "b");
    assert_eq!(BankName::try_new("b".repeat(100)).unwrap().as_str().len(), 100);
    assert!(matches!(BankName::try_new(""), Err(BankNameError::NotEmptyViolated)));
    assert!(matches!(BankName::try_new("b".repeat(101)), Err(BankNameError::LenCharMaxViolated)));
    assert!(matches!(BankName::try_new("Bank\u{202E}"), Err(BankNameError::PredicateViolated)));
}

#[test]
fn vo_16_last_four_and_hex_color_validate_and_normalize() {
    assert_eq!(LastFour::try_new(" 0042 ").unwrap().as_str(), "0042");
    for bad in ["42", "12345", "12a4"] {
        assert!(matches!(LastFour::try_new(bad), Err(LastFourError::PredicateViolated)));
    }
    assert_eq!(HexColor::try_new(" #1a2b3c ").unwrap().as_str(), "#1A2B3C");
    for bad in ["1A2B3C", "#FFF", "#12345G"] {
        assert!(matches!(HexColor::try_new(bad), Err(HexColorError::PredicateViolated)));
    }
}

#[test]
fn vo_17_year_month_validates_format_and_leap_end_of_month_utc() {
    let leap = YearMonth::try_new(" 2024-02 ").unwrap();
    let expected_leap = Utc.with_ymd_and_hms(2024, 2, 29, 23, 59, 59).unwrap()
        + Duration::microseconds(999_999);
    assert_eq!(leap.end_of_month_utc(), expected_leap);

    let non_leap = YearMonth::try_new("2026-02").unwrap();
    let expected_non_leap = Utc.with_ymd_and_hms(2026, 2, 28, 23, 59, 59).unwrap()
        + Duration::microseconds(999_999);
    assert_eq!(non_leap.end_of_month_utc(), expected_non_leap);
    assert!(Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap() > non_leap.end_of_month_utc());

    for bad in ["2026-00", "2026-13", "26-03", "2026/03"] {
        assert!(matches!(YearMonth::try_new(bad), Err(DomainError::InvalidYearMonth { .. })));
    }
}

#[test]
fn vo_18_category_tag_and_description_vos_enforce_bounds_hygiene_and_tag_cap() {
    assert_eq!(CategoryName::try_new("c".repeat(100)).unwrap().as_str().len(), 100);
    assert!(matches!(CategoryName::try_new("c".repeat(101)), Err(CategoryNameError::LenCharMaxViolated)));
    assert_eq!(CategoryIcon::try_new("i".repeat(50)).unwrap().as_str().len(), 50);
    assert!(matches!(CategoryIcon::try_new("i".repeat(51)), Err(CategoryIconError::LenCharMaxViolated)));
    assert_eq!(Tag::try_new("t".repeat(40)).unwrap().as_str().len(), 40);
    assert!(matches!(Tag::try_new("t".repeat(41)), Err(TagError::LenCharMaxViolated)));
    assert_eq!(TransactionDescription::try_new("d".repeat(500)).unwrap().as_str().len(), 500);
    assert!(matches!(
        TransactionDescription::try_new("d".repeat(501)),
        Err(TransactionDescriptionError::LenCharMaxViolated)
    ));
    assert!(matches!(
        TransactionDescription::try_new("note\0"),
        Err(TransactionDescriptionError::PredicateViolated)
    ));
    assert_eq!(MAX_TAGS_PER_TRANSACTION, 20);
    let twenty_one: Vec<String> = (1..=21).map(|i| format!("tag-{i}")).collect();
    assert_eq!(validate_tags(&twenty_one), Err(DomainError::TooManyTags { max: 20 }));
    let dedup = validate_tags([" food ", "food", "groceries"]).unwrap();
    assert_eq!(dedup.len(), 2);
}
