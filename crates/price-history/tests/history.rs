use price_history::*;

fn key(merchant: &str, name: &str, code: Option<&str>) -> ItemKey {
    ItemKey::new(merchant, name, code).unwrap()
}

fn observation(receipt: &str, item: &str, amount: i64) -> Observation {
    Observation {
        id: ObservationId {
            receipt_id: receipt.into(),
            item_id: item.into(),
        },
        item: key("Shop One", "Example Milk", Some("000042")),
        observed_name: "Example Milk".into(),
        display_name: None,
        date: Some(PurchaseDate::new(2026, 1, 1).unwrap()),
        amount_minor: Some(amount),
        currency: Some("CAD".into()),
        recorded_quantity: Some(1),
        kind: LineKind::Product,
        comparison: None,
    }
}

fn approve(observation: &mut Observation, units: u32) {
    observation.comparison = Some(ComparisonApproval {
        units,
        tax: TaxBasis::Excluded,
        discounts: DiscountBasis::BeforeDiscounts,
    });
}

fn link(aliases: Vec<ItemKey>) -> ProductLink {
    ProductLink {
        id: "milk".into(),
        display_name: "My milk".into(),
        aliases,
    }
}

fn fraction(price: UnitPrice) -> (u64, u64) {
    (price.numerator(), price.denominator())
}

#[test]
fn normalization_is_conservative_and_missing_code_is_not_a_wildcard() {
    let normalized = key("  SHOP\tONE ", "Example\n Milk", Some(" 000042 "));
    assert_eq!(normalized, key("shop one", "example milk", Some("000042")));
    assert_eq!(normalized.merchant(), "SHOP ONE");
    assert_eq!(normalized.name(), "EXAMPLE MILK");
    assert_eq!(normalized.code(), Some("000042"));
    assert_eq!(key("S", "N", Some(" \t")), key("s", "n", None));
    let mut rows = Vec::new();
    for (index, item) in [
        normalized,
        key("shop one", "example milk", Some("000042")),
        key("shop one", "example milk", None),
        key("shop one", "example milk", Some("42")),
        key("shop one", "example milk", Some("000043")),
        key("shop two", "example milk", Some("000042")),
        key("shop one", "example-milk", Some("000042")),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = observation(&index.to_string(), "line", 349);
        row.item = item;
        rows.push(row);
    }
    let groups = build_history(&rows, &[]).unwrap();
    assert_eq!(groups.len(), 6);
    assert_eq!(groups.iter().map(|g| g.entries.len()).sum::<usize>(), 7);
    assert_ne!(key("S", "N", Some("ab")), key("S", "N", Some("AB")));
    assert_eq!(
        ItemKey::new(" ", "milk", None),
        Err(HistoryError::EmptyItemKey)
    );
    assert_eq!(
        ItemKey::new("shop", "\n", None),
        Err(HistoryError::EmptyItemKey)
    );
}

#[test]
fn repeated_lines_survive_and_exact_duplicate_ids_are_rejected() {
    let a = observation("receipt1", "line1", 349);
    let b = observation("receipt1", "line2", 349);
    let c = observation("receipt2", "line1", 349);
    let groups = build_history(&[a.clone(), b, c], &[]).unwrap();
    assert_eq!(groups[0].entries.len(), 3);
    assert_eq!(groups[0].receipt_count, 2);
    assert_eq!(
        build_history(&[a.clone(), a.clone()], &[]),
        Err(HistoryError::DuplicateObservation(a.id.clone()))
    );
    let mut changed = a.clone();
    changed.amount_minor = Some(400);
    assert_eq!(
        build_history(&[a.clone(), changed], &[]),
        Err(HistoryError::DuplicateObservation(a.id))
    );
}

#[test]
fn explicit_aliases_link_stores_and_undo_without_rewriting_evidence() {
    let mut a = observation("r1", "line1", 349);
    let mut b = observation("r2", "line1", 399);
    b.item = key("Shop Two", "EX MPL MLK", Some("888"));
    b.observed_name = "EX MPL MLK".into();
    b.display_name = Some("Breakfast milk".into());
    approve(&mut a, 1);
    approve(&mut b, 1);
    let rows = vec![a.clone(), b.clone()];
    let separate = build_history(&rows, &[]).unwrap();
    assert_eq!(separate.len(), 2);
    let unused_alias = key("Shop Three", "Old product name", None);
    let product = link(vec![a.item.clone(), b.item.clone(), unused_alias]);
    let joined = build_history(&rows, &[product]).unwrap();
    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].key, HistoryKey::Product("milk".into()));
    assert_eq!(joined[0].display_name, "My milk");
    assert_eq!(joined[0].entries[0].observation, a);
    assert_eq!(joined[0].entries[1].observation, b);
    assert_eq!(joined[0].comparisons.len(), 2); // Never averages across stores.
    for query in [
        "my milk",
        "ex mpl",
        "breakfast",
        "888",
        "shop two",
        "old product",
        "",
    ] {
        assert_eq!(search(&joined, query).len(), 1, "{query}");
    }
    assert!(search(&joined, "unrelated").is_empty());
    assert_eq!(build_history(&rows, &[]).unwrap(), separate);
}

#[test]
fn conflicting_aliases_fail_independent_of_product_order() {
    let item = key("SHOP", "Milk", None);
    let a = link(vec![item.clone()]);
    let mut b = a.clone();
    b.id = "other".into();
    for products in [vec![a.clone(), b.clone()], vec![b, a.clone()]] {
        assert_eq!(
            build_history(&[], &products),
            Err(HistoryError::ConflictingAlias(item.clone()))
        );
    }
    assert_eq!(
        build_history(&[], &[a.clone(), a]),
        Err(HistoryError::DuplicateProductId("milk".into()))
    );
}

#[test]
fn linking_product_identity_does_not_approve_prices() {
    let a = observation("r1", "line", 349);
    let mut b = observation("r2", "line", 698);
    b.item = key("Shop Two", "Different printed name", Some("999"));
    let product = link(vec![a.item.clone(), b.item.clone()]);
    let groups = build_history(&[a.clone(), b.clone()], std::slice::from_ref(&product)).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].entries.len(), 2);
    assert!(groups[0].comparisons.is_empty());

    // Approval is per observation. A reviewed alias does not approve the other
    // store's potentially multi-unit or mispaired amount.
    approve(&mut b, 2);
    let groups = build_history(&[a, b], &[product]).unwrap();
    assert_eq!(groups[0].comparisons.len(), 1);
    assert_eq!(groups[0].comparisons[0].key.merchant, "SHOP TWO");
    assert_eq!(fraction(groups[0].comparisons[0].average), (349, 1));
    assert_eq!(groups[0].comparisons[0].points.len(), 1);
}

#[test]
fn repeated_alias_within_one_product_is_harmless_and_canonicalized() {
    let a = observation("r", "i", 100);
    let product = link(vec![a.item.clone(), a.item.clone()]);
    let groups = build_history(&[a], &[product]).unwrap();
    assert_eq!(groups[0].aliases.len(), 1);
}

#[test]
fn recorded_quantity_one_does_not_turn_a_line_amount_into_a_unit_price() {
    let mut single = observation("r1", "i", 349);
    let mut double = observation("r2", "i", 698);
    double.date = Some(PurchaseDate::new(2026, 2, 1).unwrap());
    // Both parser outputs say one. Neither supports a claimed price doubling.
    let groups = build_history(&[single.clone(), double.clone()], &[]).unwrap();
    assert!(groups[0].comparisons.is_empty());
    assert!(groups[0]
        .entries
        .iter()
        .all(|e| e.comparison_issues == [ComparisonIssue::Unapproved]));
    assert_eq!(groups[0].entries[1].observation.amount_minor, Some(698));
    approve(&mut single, 1);
    approve(&mut double, 2);
    let groups = build_history(&[single, double], &[]).unwrap();
    let series = &groups[0].comparisons[0];
    assert_eq!(fraction(series.minimum), (349, 1));
    assert_eq!(series.minimum, series.maximum);
    assert_eq!(fraction(series.average), (349, 1));
    assert_eq!(series.latest[0].units, 2);
    assert_eq!(groups[0].entries[1].observation.recorded_quantity, Some(1));
}

#[test]
fn unknown_fields_and_nonproducts_survive_with_explicit_comparison_issues() {
    let mut missing = observation("missing", "i", 100);
    missing.date = None;
    missing.currency = None;
    missing.amount_minor = None;
    let mut rows = vec![missing];
    for (index, kind) in [
        LineKind::Discount,
        LineKind::Deposit,
        LineKind::Return,
        LineKind::GiftCard,
        LineKind::Other,
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = observation(&index.to_string(), "i", -100);
        row.kind = kind;
        approve(&mut row, 1);
        rows.push(row);
    }
    let mut negative = observation("negative-product", "i", -100);
    approve(&mut negative, 1);
    rows.push(negative);
    let groups = build_history(&rows, &[]).unwrap();
    assert_eq!(groups[0].entries.len(), 7);
    assert!(groups[0].comparisons.is_empty());
    let issues = &groups[0].entries.last().unwrap().comparison_issues;
    assert_eq!(
        issues,
        &[
            ComparisonIssue::UnknownDate,
            ComparisonIssue::UnknownCurrency,
            ComparisonIssue::UnreadableAmount,
            ComparisonIssue::Unapproved
        ]
    );
    assert!(groups[0].entries[0]
        .comparison_issues
        .contains(&ComparisonIssue::NotProduct));
}

#[test]
fn generic_labels_remain_purchase_lists_without_approval() {
    let mut a = observation("r1", "i", 200);
    a.item = key("Shop", "MEAT", None);
    let mut b = a.clone();
    b.id.receipt_id = "r2".into();
    b.amount_minor = Some(2000);
    let groups = build_history(&[a, b], &[]).unwrap();
    assert_eq!(groups[0].entries.len(), 2);
    assert!(groups[0].comparisons.is_empty());
}

#[test]
fn currencies_and_price_bases_form_separate_series() {
    let mut rows = Vec::new();
    for currency in [" cad ", "USD"] {
        for tax in [TaxBasis::Included, TaxBasis::Excluded] {
            for discounts in [
                DiscountBasis::BeforeDiscounts,
                DiscountBasis::AfterDiscounts,
            ] {
                let mut row = observation(&rows.len().to_string(), "i", 100);
                row.currency = Some(currency.into());
                row.comparison = Some(ComparisonApproval {
                    units: 1,
                    tax,
                    discounts,
                });
                rows.push(row);
            }
        }
    }
    let groups = build_history(&rows, &[]).unwrap();
    assert_eq!(groups[0].comparisons.len(), 8);
    assert!(groups[0].comparisons.iter().all(|s| s.points.len() == 1));
    assert_eq!(groups[0].comparisons[0].key.currency, "CAD");
}

#[test]
fn statistics_are_exact_and_average_is_weighted_by_confirmed_units() {
    let mut a = observation("a", "i", 600);
    let mut b = observation("b", "i", 100);
    approve(&mut a, 2);
    approve(&mut b, 1);
    let groups = build_history(&[a, b], &[]).unwrap();
    let series = &groups[0].comparisons[0];
    assert_eq!(fraction(series.minimum), (100, 1));
    assert_eq!(fraction(series.maximum), (300, 1));
    assert_eq!(fraction(series.average), (700, 3));
    let mut tiny = observation("c", "i", 1);
    approve(&mut tiny, 3);
    let groups = build_history(&[tiny], &[]).unwrap();
    assert_eq!(fraction(groups[0].comparisons[0].average), (1, 3));
}

#[test]
fn free_items_are_zero_not_unreadable_and_large_prices_do_not_lose_precision() {
    let mut free = observation("free", "i", 0);
    let mut high = observation("high", "i", i64::MAX);
    let mut lower = observation("lower", "i", i64::MAX - 1);
    approve(&mut free, 3);
    approve(&mut high, 1);
    approve(&mut lower, 1);
    let groups = build_history(&[free, high, lower], &[]).unwrap();
    let series = &groups[0].comparisons[0];
    assert_eq!(fraction(series.minimum), (0, 1));
    assert_eq!(fraction(series.maximum), (i64::MAX as u64, 1));
    assert!(series.points[1].unit_price > series.points[2].unit_price);
    assert_eq!(fraction(series.average), ((i64::MAX as u64) * 2 - 1, 5));
}

#[test]
fn aggregate_overflow_is_an_error_instead_of_wrapping_or_saturating() {
    let mut rows = Vec::new();
    for id in ["a", "b", "c"] {
        let mut row = observation(id, "i", i64::MAX);
        approve(&mut row, 1);
        rows.push(row);
    }
    assert_eq!(
        build_history(&rows, &[]),
        Err(HistoryError::ArithmeticOverflow)
    );
}

#[test]
fn latest_preserves_same_day_ties_and_unknown_dates_never_win() {
    let mut a = observation("a", "i", 100);
    let mut b = observation("b", "i", 200);
    let mut c = observation("c", "i", 300);
    let mut undated = observation("z", "i", 9999);
    a.date = Some(PurchaseDate::new(2025, 12, 31).unwrap());
    undated.date = None;
    for row in [&mut a, &mut b, &mut c, &mut undated] {
        approve(row, 1);
    }
    let groups = build_history(&[undated, c, a, b], &[]).unwrap();
    let group = &groups[0];
    let ids: Vec<_> = group
        .entries
        .iter()
        .map(|e| e.observation.id.receipt_id.as_str())
        .collect();
    assert_eq!(ids, ["a", "b", "c", "z"]);
    assert_eq!(group.comparisons[0].latest.len(), 2);
    assert_eq!(fraction(group.comparisons[0].maximum), (300, 1));
}

#[test]
fn input_order_does_not_change_history_and_rebuilding_applies_edits_and_deletions() {
    let mut a = observation("a", "i", 100);
    let mut b = observation("b", "i", 200);
    approve(&mut a, 1);
    approve(&mut b, 1);
    assert_eq!(
        build_history(&[a.clone(), b.clone()], &[]),
        build_history(&[b.clone(), a.clone()], &[])
    );
    a.amount_minor = Some(300);
    let groups = build_history(&[a], &[]).unwrap();
    assert_eq!(groups[0].receipt_count, 1);
    assert_eq!(fraction(groups[0].comparisons[0].minimum), (300, 1));
    assert!(build_history(&[], &[]).unwrap().is_empty());
}

#[test]
fn malformed_input_fails_before_producing_partial_histories() {
    let mut row = observation("", "i", 100);
    assert_eq!(
        build_history(&[row.clone()], &[]),
        Err(HistoryError::EmptyObservationId)
    );
    row.id.receipt_id = "r".into();
    for currency in ["", "CA", "123", "C AD", "CÄD"] {
        row.currency = Some(currency.into());
        assert_eq!(
            build_history(&[row.clone()], &[]),
            Err(HistoryError::InvalidCurrency(row.id.clone()))
        );
    }
    row.currency = Some("CAD".into());
    approve(&mut row, 0);
    assert_eq!(
        build_history(&[row.clone()], &[]),
        Err(HistoryError::ZeroApprovedUnits(row.id))
    );
    let invalid = link(vec![]);
    assert_eq!(
        build_history(&[], &[invalid]),
        Err(HistoryError::InvalidProductLink("milk".into()))
    );
}

#[test]
fn calendar_dates_validate_centuries_and_month_lengths() {
    for (year, month, day) in [
        (0, 1, 1),
        (10000, 1, 1),
        (2026, 0, 1),
        (2026, 13, 1),
        (2026, 1, 0),
        (2026, 4, 31),
        (2026, 2, 29),
        (1900, 2, 29),
    ] {
        assert_eq!(
            PurchaseDate::new(year, month, day),
            Err(HistoryError::InvalidDate)
        );
    }
    let date = PurchaseDate::new(2000, 2, 29).unwrap();
    assert_eq!(date.to_string(), "2000-02-29");
    assert_eq!((date.year(), date.month(), date.day()), (2000, 2, 29));
    assert!(PurchaseDate::new(2024, 2, 29).is_ok());
}
