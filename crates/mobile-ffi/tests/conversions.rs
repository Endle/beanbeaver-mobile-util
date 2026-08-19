//! The seam's own risk is not arithmetic — `spend-core` owns that and pins it
//! with 28 tests. It is the ~40 fields copied by hand between the mirrored
//! records, where a transposition compiles perfectly and quietly reports the
//! wrong number on a screen.
//!
//! So these assert values that are all *distinct*: swap any two fields of the
//! same type and a test fails. `tracked` / `items_total` / `tax` /
//! `receipt_total` are the dangerous quartet — four `f64`s in a row — and are
//! deliberately given four different values.

use bb_mobile_ffi::{
    spend_declared_roots, spend_items, spend_month, spend_month_id, spend_receipt_groups,
    spend_resolve_budget_root, spend_trend, SpendCategory, SpendDate, SpendInput, SpendItem,
    SpendTag,
};

fn tag(path: &str, display: &str) -> SpendTag {
    SpendTag {
        path: path.into(),
        display: display.into(),
    }
}

fn item(description: &str, price: &str, tags: Vec<SpendTag>) -> SpendItem {
    SpendItem {
        description: description.into(),
        price: price.into(),
        tags,
    }
}

fn record(id: &str, total: &str, tax: Option<&str>, items: Vec<SpendItem>) -> SpendInput {
    SpendInput {
        id: id.into(),
        date_iso: Some("2026-07-15".into()),
        date_is_placeholder: false,
        scanned_on: SpendDate {
            year: 2026,
            month: 7,
            day: 20,
        },
        is_excluded: false,
        total: total.into(),
        tax: tax.map(Into::into),
        items,
    }
}

/// Every `f64` and every count distinct, so a swapped pair cannot pass.
#[test]
fn month_fields_survive_the_round_trip_unswapped() {
    let records = vec![record(
        "r1",
        "31.00", // receipt_total: distinct from tracked (27) on purpose
        Some("2.00"),
        vec![
            item(
                "MILK",
                "10.00",
                vec![tag("grocery", "Grocery"), tag("grocery/dairy", "Dairy")],
            ),
            item("PAPER TOWELS", "15.00", vec![tag("household", "Household")]),
            item("SMUDGED", "N/A", vec![tag("grocery", "Grocery")]),
        ],
    )];

    let m = spend_month("2026-07".into(), records);

    assert_eq!("2026-07", m.id);
    assert_eq!("July 2026", m.label);
    assert_eq!(2026, m.year);
    assert_eq!(7, m.month);
    assert_eq!(25.0, m.items_total);
    assert_eq!(2.0, m.tax);
    assert_eq!(27.0, m.tracked);
    assert_eq!(31.0, m.receipt_total);
    assert_eq!(Some(-4.0), m.unaccounted);
    assert_eq!(1, m.receipt_count);
    assert_eq!(0, m.excluded_count);
    assert_eq!(1, m.unreadable_price_count);
    assert_eq!(vec!["r1".to_string()], m.record_ids);
    assert_eq!(15.0, m.max_leaf_amount);

    // Roots largest first, and the leaf nesting survives.
    let ids: Vec<&str> = m.roots.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(vec!["household", "grocery"], ids);
    let grocery = m.roots.iter().find(|r| r.id == "grocery").expect("grocery");
    assert_eq!("Grocery", grocery.label);
    assert_eq!(10.0, grocery.amount);
    assert_eq!(2, grocery.item_count); // the unreadable one still counts
    let dairy = grocery
        .leaves
        .iter()
        .find(|l| l.label == "Dairy")
        .expect("dairy leaf");
    assert_eq!(10.0, dairy.amount);
    assert_eq!(1, dairy.item_count);
}

/// `date_is_placeholder` is the one input field with no natural default, and
/// getting it backwards would silently rebucket every receipt.
#[test]
fn the_placeholder_flag_crosses_the_seam_the_right_way_round() {
    let mut r = record("r1", "0.00", None, vec![]);
    assert_eq!("2026-07", spend_month_id(clone_input(&r))); // receipt date wins
    r.date_iso = Some("2026-03-02".into());
    assert_eq!("2026-03", spend_month_id(clone_input(&r)));
    r.date_is_placeholder = true;
    assert_eq!("2026-07", spend_month_id(r)); // falls back to scanned_on
}

/// Distinct index, id and description, so the drill-down's three string-ish
/// fields cannot be confused for one another.
#[test]
fn item_entries_carry_an_index_that_matches_their_position() {
    let records = vec![record(
        "rec-a",
        "14.00",
        None,
        vec![
            item("MILK", "10.00", vec![tag("grocery/dairy", "Dairy")]),
            item("BREAD", "1.00", vec![tag("grocery/bakery", "Bakery")]),
            item("YOGURT", "4.00", vec![tag("grocery/dairy", "Dairy")]),
        ],
    )];

    let entries = spend_items(
        SpendCategory::Leaf {
            label: "Dairy".into(),
        },
        records.clone_all(),
    );
    assert_eq!(2, entries.len());
    assert_eq!("MILK", entries[0].description);
    assert_eq!(0, entries[0].item_index);
    assert_eq!("rec-a-0", entries[0].id);
    assert_eq!("rec-a", entries[0].record_id);
    assert_eq!(10.0, entries[0].amount);
    // Index 2, not 1: it is the position in the receipt, not in the result.
    assert_eq!("YOGURT", entries[1].description);
    assert_eq!(2, entries[1].item_index);
    assert_eq!("rec-a-2", entries[1].id);

    let groups = spend_receipt_groups(
        SpendCategory::Leaf {
            label: "Dairy".into(),
        },
        records.clone_all(),
    );
    assert_eq!(1, groups.len());
    assert_eq!("rec-a", groups[0].record_id);
    assert_eq!(14.0, groups[0].amount);
    assert_eq!(Some(14.0), groups[0].receipt_total);
}

#[test]
fn the_budget_root_rule_crosses_the_seam() {
    let tags = vec![
        tag("household", "Household"),
        tag("grocery/dairy", "Dairy"),
        tag("grocery", "Grocery"),
    ];
    assert_eq!(vec!["household", "grocery"], spend_declared_roots(tags));
    let declared = vec!["household".to_string(), "grocery".to_string()];
    assert_eq!("grocery", spend_resolve_budget_root(None, declared.clone()));
    assert_eq!(
        "household",
        spend_resolve_budget_root(Some("household".into()), declared)
    );
}

#[test]
fn the_trend_crosses_the_seam_with_its_ranges_intact() {
    // Four `f64`s again — `mean`, `delta`, `rolling` and the newest point — so
    // they are given four different values, and the two `SpendDateRange`s are
    // checked separately because both are the same type.
    let dated = |id: &str, iso: &str, price: &str| SpendInput {
        date_iso: Some(iso.into()),
        ..record(id, price, None, vec![item("Milk", price, vec![])])
    };
    let records = vec![
        dated("prev", "2026-02-25", "30.00"),
        dated("now", "2026-03-04", "45.20"),
    ];

    let today = SpendDate {
        year: 2026,
        month: 3,
        day: 4,
    };
    let trend = spend_trend(records, None, today, 1, 6, 30);

    assert_eq!(6, trend.points.len());
    let newest = trend.points.last().expect("six weeks");
    assert_eq!(45.20, newest.amount);
    assert_eq!(2026, newest.range.start.year);
    assert_eq!(3, newest.range.start.month);
    assert_eq!(1, newest.range.start.day);
    assert_eq!(8, newest.range.end.day);

    assert_eq!(Some(15.20), trend.delta);
    assert_eq!(12.53, trend.mean); // (30.00 + 45.20) / 6, to cents
    assert_eq!(75.20, trend.rolling);
    assert_eq!(2, trend.rolling_range.start.month);
    assert_eq!(3, trend.rolling_range.start.day);
    assert_eq!(5, trend.rolling_range.end.day);
}

#[test]
fn a_scope_crosses_the_seam_as_the_category_it_names() {
    let records = vec![record(
        "mixed",
        "30.00",
        None,
        vec![
            item("Milk", "10.00", vec![tag("grocery", "Grocery")]),
            item("Soap", "20.00", vec![tag("household", "Household")]),
        ],
    )];
    let today = SpendDate {
        year: 2026,
        month: 7,
        day: 15,
    };
    let scope = SpendCategory::Root {
        id: "household".into(),
    };
    let later = SpendDate {
        year: today.year,
        month: today.month,
        day: today.day,
    };
    let trend = spend_trend(records.clone_all(), Some(scope), today, 1, 6, 30);
    assert_eq!(20.00, trend.rolling);

    let unscoped = spend_trend(records, None, later, 1, 6, 30);
    assert_eq!(30.00, unscoped.rolling);
}

// The uniffi records are deliberately not `Clone` — they are FFI payloads, moved
// once. These helpers keep the tests readable without widening the public API.
fn clone_input(r: &SpendInput) -> SpendInput {
    SpendInput {
        id: r.id.clone(),
        date_iso: r.date_iso.clone(),
        date_is_placeholder: r.date_is_placeholder,
        scanned_on: SpendDate {
            year: r.scanned_on.year,
            month: r.scanned_on.month,
            day: r.scanned_on.day,
        },
        is_excluded: r.is_excluded,
        total: r.total.clone(),
        tax: r.tax.clone(),
        items: r
            .items
            .iter()
            .map(|i| SpendItem {
                description: i.description.clone(),
                price: i.price.clone(),
                tags: i.tags.iter().map(|t| tag(&t.path, &t.display)).collect(),
            })
            .collect(),
    }
}

trait CloneAll {
    fn clone_all(&self) -> Vec<SpendInput>;
}

impl CloneAll for Vec<SpendInput> {
    fn clone_all(&self) -> Vec<SpendInput> {
        self.iter().map(clone_input).collect()
    }
}
