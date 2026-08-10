//! The specification for `spend-core`, ported one-for-one from
//! `beanbeaver-android`'s `SpendSummaryTest.kt` (20 JVM tests), which in turn
//! asserts the *behaviour* the iOS twin documents rather than either
//! implementation. Each test keeps its original name and its original comment,
//! so a divergence is traceable back to the app it came from.
//!
//! Two tests are stronger here than in Kotlin, both because this crate has no
//! clock: `with nothing scanned the default month is the current one` pins a
//! fixed "today" instead of calling `LocalDate.now()` and comparing two live
//! reads, and every scan-time fallback is an explicit calendar date rather than
//! epoch millis reinterpreted through the host's timezone.

use spend_core::{
    declared_roots, default_month_id, items, leaf_label, month, month_id, month_ids, month_label,
    price_value, receipt_groups, resolve_budget_root, Category, ItemTag, SpendDate, SpendInput,
    SpendItem, FALLBACK_BUDGET_ROOT, UNCATEGORIZED_ROOT,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn tag(path: &str, display: &str) -> ItemTag {
    ItemTag::new(path, display)
}

fn item(description: &str, price: &str, tags: Vec<ItemTag>) -> SpendItem {
    SpendItem {
        description: description.into(),
        price: price.into(),
        tags,
    }
}

fn grocery() -> ItemTag {
    tag("grocery", "Grocery")
}
fn dairy() -> ItemTag {
    tag("grocery/dairy", "Dairy")
}
fn household() -> ItemTag {
    tag("household", "Household")
}
fn supply() -> ItemTag {
    tag("household/supply", "Supply")
}

/// Mirrors the Kotlin fixture's defaults: `date = "2026-07-15"`, scanned
/// 2026-07-20, not excluded, total "0.00", no tax.
struct Builder(SpendInput);

fn record(id: &str) -> Builder {
    Builder(SpendInput {
        id: id.into(),
        date_iso: Some("2026-07-15".into()),
        date_is_placeholder: false,
        scanned_on: SpendDate::new(2026, 7, 20),
        is_excluded: false,
        total: "0.00".into(),
        tax: None,
        items: Vec::new(),
    })
}

impl Builder {
    fn date(mut self, iso: Option<&str>) -> Self {
        self.0.date_iso = iso.map(Into::into);
        self
    }
    fn placeholder(mut self) -> Self {
        self.0.date_is_placeholder = true;
        self
    }
    fn scanned_on(mut self, y: i32, m: u32, d: u32) -> Self {
        self.0.scanned_on = SpendDate::new(y, m, d);
        self
    }
    fn excluded(mut self) -> Self {
        self.0.is_excluded = true;
        self
    }
    fn total(mut self, v: &str) -> Self {
        self.0.total = v.into();
        self
    }
    fn tax(mut self, v: &str) -> Self {
        self.0.tax = Some(v.into());
        self
    }
    fn items(mut self, v: Vec<SpendItem>) -> Self {
        self.0.items = v;
        self
    }
    fn build(self) -> SpendInput {
        self.0
    }
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 0.001, "expected {b}, got {a}");
}

fn descriptions(entries: &[spend_core::ItemEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.description.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Month bucketing
// ---------------------------------------------------------------------------

/// The receipt's own date wins — that's the day the money was spent.
#[test]
fn a_record_buckets_by_its_receipt_date() {
    let r = record("r1")
        .date(Some("2026-03-02"))
        .scanned_on(2026, 7, 20)
        .build();
    assert_eq!("2026-03", month_id(&r));
}

/// A placeholder date falls back to the row's *own* scan time, not "today", so a
/// bucket can't drift with the clock on a later run.
#[test]
fn a_placeholder_date_falls_back_to_the_scan_time_not_today() {
    let r = record("r1")
        .date(Some("2026-03-02"))
        .placeholder()
        .scanned_on(2026, 7, 20)
        .build();
    assert_eq!("2026-07", month_id(&r));
}

#[test]
fn a_missing_date_falls_back_to_the_scan_time() {
    let r = record("r1").date(None).scanned_on(2026, 5, 4).build();
    assert_eq!("2026-05", month_id(&r));
}

/// Not in the Kotlin suite, and it should be: an unparseable date takes the same
/// path as a missing one, and nothing pinned that.
#[test]
fn an_unparseable_date_falls_back_to_the_scan_time() {
    for bad in ["2026-13-01", "2026-03-00", "not-a-date", "2026-3-2", ""] {
        let r = record("r1").date(Some(bad)).scanned_on(2026, 5, 4).build();
        assert_eq!("2026-05", month_id(&r), "input {bad:?}");
    }
}

#[test]
fn months_are_listed_newest_first() {
    let records = vec![
        record("a").date(Some("2026-05-01")).build(),
        record("b").date(Some("2026-07-01")).build(),
        record("c").date(Some("2026-06-01")).build(),
        record("d").date(Some("2026-07-20")).build(),
    ];
    assert_eq!(vec!["2026-07", "2026-06", "2026-05"], month_ids(&records));
}

/// Deliberately *not* "the current month": a screen opening on a $0.00 month
/// because the last receipt was in September shows nothing and looks broken.
#[test]
fn the_default_month_is_the_newest_with_receipts_not_the_current_one() {
    let records = vec![record("r1").date(Some("2020-01-15")).build()];
    assert_eq!(
        "2020-01",
        default_month_id(&records, SpendDate::new(2026, 7, 20))
    );
}

#[test]
fn with_nothing_scanned_the_default_month_is_the_current_one() {
    assert_eq!(
        "2026-07",
        default_month_id(&[], SpendDate::new(2026, 7, 20))
    );
}

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

#[test]
fn tracked_is_items_plus_tax_and_roots_sum_to_items() {
    let records = vec![record("r1")
        .total("27.00")
        .tax("2.00")
        .items(vec![
            item("MILK", "10.00", vec![grocery(), dairy()]),
            item("PAPER TOWELS", "15.00", vec![household(), supply()]),
        ])
        .build()];
    let m = month("2026-07", &records);
    approx(m.items_total, 25.0);
    approx(m.tax, 2.0);
    approx(m.tracked, 27.0);
    approx(m.roots.iter().map(|r| r.amount).sum(), 25.0);
    // Items + tax landed exactly on the receipt total, so there is no gap.
    assert_eq!(None, m.unaccounted);
}

/// The reconciliation row exists for exactly this: a scan that read every item
/// but missed a discount line. It's named rather than hidden, because otherwise
/// it looks like arithmetic the app got wrong.
#[test]
fn a_gap_against_the_receipt_total_is_reported_as_unaccounted() {
    let records = vec![record("r1")
        .total("20.00")
        .items(vec![item("MILK", "25.00", vec![grocery(), dairy()])])
        .build()];
    approx(month("2026-07", &records).unaccounted.expect("a gap"), 5.0);
}

#[test]
fn roots_and_leaves_are_ordered_largest_first() {
    let records = vec![record("r1")
        .items(vec![
            item("MILK", "5.00", vec![grocery(), dairy()]),
            item("YOGURT", "9.00", vec![grocery(), dairy()]),
            item(
                "BREAD",
                "3.00",
                vec![grocery(), tag("grocery/bakery", "Bakery")],
            ),
            item("PAPER TOWELS", "40.00", vec![household(), supply()]),
        ])
        .build()];
    let m = month("2026-07", &records);
    assert_eq!(
        vec!["household", "grocery"],
        m.roots.iter().map(|r| r.id.as_str()).collect::<Vec<_>>()
    );
    let leaves = &m.group("grocery").expect("grocery group").leaves;
    assert_eq!(
        vec!["Dairy", "Bakery"],
        leaves.iter().map(|l| l.label.as_str()).collect::<Vec<_>>()
    );
    approx(leaves[0].amount, 14.0);
    assert_eq!(2, leaves[0].item_count);
}

/// The vocabulary's own wording beats capitalizing a raw path segment — the
/// reason `personalcare` reads as "Personal Care" and not "Personalcare".
#[test]
fn a_root_takes_its_authored_label_when_an_item_carries_the_root_tag() {
    let records = vec![record("r1")
        .items(vec![item(
            "SHAMPOO",
            "8.00",
            vec![tag("personalcare", "Personal Care")],
        )])
        .build()];
    let m = month("2026-07", &records);
    assert_eq!(1, m.roots.len());
    assert_eq!("Personal Care", m.roots[0].label);
}

/// Untagged items stay visible, so the breakdown reconciles against what was
/// scanned.
#[test]
fn untagged_items_land_in_a_real_uncategorized_group() {
    let records = vec![record("r1")
        .items(vec![item("MYSTERY", "3.00", vec![])])
        .build()];
    let m = month("2026-07", &records);
    assert_eq!(1, m.roots.len());
    assert_eq!(UNCATEGORIZED_ROOT, m.roots[0].id);
    assert_eq!("Uncategorized", m.roots[0].label);
    approx(m.items_total, 3.0);
}

/// An unreadable price is counted and carried at zero rather than dropped: the
/// item still happened, and the footer says how many couldn't be read.
#[test]
fn an_unreadable_price_is_counted_not_silently_treated_as_free() {
    let records = vec![record("r1")
        .items(vec![
            item("MILK", "10.00", vec![grocery(), dairy()]),
            item("SMUDGED", "N/A", vec![grocery(), dairy()]),
        ])
        .build()];
    let m = month("2026-07", &records);
    assert_eq!(1, m.unreadable_price_count);
    approx(m.items_total, 10.0);
    assert_eq!(2, m.group("grocery").expect("grocery group").item_count);
}

#[test]
fn an_excluded_receipt_is_counted_but_contributes_nothing() {
    let records = vec![
        record("a")
            .total("10.00")
            .items(vec![item("MILK", "10.00", vec![grocery(), dairy()])])
            .build(),
        record("b")
            .total("99.00")
            .items(vec![item("WINE", "99.00", vec![grocery()])])
            .excluded()
            .build(),
    ];
    let m = month("2026-07", &records);
    approx(m.items_total, 10.0);
    assert_eq!(1, m.excluded_count);
    assert_eq!(1, m.receipt_count);
    // `record_ids` keeps the excluded row: the Receipts list still shows it.
    assert_eq!(2, m.record_ids.len());
}

/// One scale for every bar on the screen, so two cards are actually comparable.
#[test]
fn max_leaf_amount_spans_every_root() {
    let records = vec![record("r1")
        .items(vec![
            item("MILK", "5.00", vec![grocery(), dairy()]),
            item("PAPER TOWELS", "40.00", vec![household(), supply()]),
        ])
        .build()];
    approx(month("2026-07", &records).max_leaf_amount, 40.0);
}

// ---------------------------------------------------------------------------
// Drill-down
// ---------------------------------------------------------------------------

/// A root matches on its **raw tag id**. Matching on the display label would
/// drop every item in the group that didn't itself carry the root tag.
#[test]
fn a_root_drill_down_catches_items_that_never_carried_the_root_tag() {
    let records = vec![record("r1")
        .items(vec![
            item("MILK", "10.00", vec![grocery(), dairy()]),
            // Only the leaf tag — no bare "grocery" on this line.
            item("BREAD", "4.00", vec![tag("grocery/bakery", "Bakery")]),
        ])
        .build()];
    let entries = items(&Category::Root("grocery".into()), &records);
    assert_eq!(vec!["MILK", "BREAD"], descriptions(&entries));
    approx(entries.iter().map(|e| e.amount).sum(), 14.0);
}

#[test]
fn a_leaf_drill_down_matches_the_label_the_total_was_accumulated_under() {
    let records = vec![record("r1")
        .items(vec![
            item("MILK", "10.00", vec![grocery(), dairy()]),
            item(
                "BREAD",
                "4.00",
                vec![grocery(), tag("grocery/bakery", "Bakery")],
            ),
        ])
        .build()];
    let entries = items(&Category::Leaf("Dairy".into()), &records);
    assert_eq!(vec!["MILK"], descriptions(&entries));
}

/// The grouping is folded from the flat list, so a receipt's share can never
/// disagree with the category total that was tapped to reach it.
#[test]
fn receipt_groups_sum_to_the_category_total() {
    let records = vec![
        record("a")
            .total("14.00")
            .items(vec![
                item("MILK", "10.00", vec![grocery(), dairy()]),
                item("YOGURT", "4.00", vec![grocery(), dairy()]),
            ])
            .build(),
        record("b")
            .total("6.00")
            .items(vec![item("CHEESE", "6.00", vec![grocery(), dairy()])])
            .build(),
    ];
    let category = Category::Leaf("Dairy".into());
    let groups = receipt_groups(&category, &records);
    assert_eq!(2, groups.len());
    assert_eq!(
        vec![2, 1],
        groups.iter().map(|g| g.entries.len()).collect::<Vec<_>>()
    );
    approx(groups[0].amount, 14.0);
    approx(
        groups.iter().map(|g| g.amount).sum(),
        items(&category, &records).iter().map(|e| e.amount).sum(),
    );
    // The receipt's own total is context only — never what the group spends.
    approx(groups[0].receipt_total.expect("parsed total"), 14.0);
}

#[test]
fn an_excluded_receipt_is_absent_from_the_drill_down_too() {
    let records = vec![
        record("a")
            .items(vec![item("MILK", "10.00", vec![grocery(), dairy()])])
            .build(),
        record("b")
            .items(vec![item("CHEESE", "6.00", vec![grocery(), dairy()])])
            .excluded()
            .build(),
    ];
    let entries = items(&Category::Leaf("Dairy".into()), &records);
    assert_eq!(vec!["MILK"], descriptions(&entries));
}

/// Two identical lines on one receipt have to stay distinct rows.
#[test]
fn duplicate_lines_on_one_receipt_get_distinct_entry_ids() {
    let records = vec![record("r1")
        .items(vec![
            item("MILK", "6.69", vec![grocery(), dairy()]),
            item("MILK", "6.69", vec![grocery(), dairy()]),
        ])
        .build()];
    let ids: Vec<String> = items(&Category::Leaf("Dairy".into()), &records)
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(2, ids.len());
    assert_eq!(
        2,
        ids.iter().collect::<std::collections::HashSet<_>>().len()
    );
}

#[test]
fn month_labels_read_the_way_a_person_writes_them() {
    assert!(month_label("2026-07").contains("2026"));
    // Unparseable input falls back to the raw id rather than vanishing.
    assert_eq!("not-a-month", month_label("not-a-month"));
}

// ---------------------------------------------------------------------------
// Helpers the arithmetic travels with. Not separately pinned in the Kotlin
// suite — `priceValue` and `tagDisplay` live in Format.kt and are covered
// indirectly — but they decide several figures above, so they get their own
// asserts here rather than being reachable only through a month total.
// ---------------------------------------------------------------------------

#[test]
fn price_value_reads_money_and_refuses_what_it_cannot() {
    approx(price_value("$8.42").expect("8.42"), 8.42);
    approx(price_value("-$3.50").expect("-3.50"), -3.5);
    approx(price_value("1,234.50").expect("1234.50"), 1234.50);
    approx(price_value(".5").expect(".5"), 0.5);
    for bad in ["N/A", "", "--", "-", ".", "1.2.3", "abc"] {
        assert_eq!(None, price_value(bad), "input {bad:?}");
    }
}

#[test]
fn leaf_label_takes_the_last_authored_display() {
    assert_eq!("Dairy", leaf_label(&[grocery(), dairy()]));
    // An empty display is skipped rather than winning by being last.
    assert_eq!("Grocery", leaf_label(&[grocery(), tag("grocery/x", "")]));
    assert_eq!("Uncategorized", leaf_label(&[]));
    assert_eq!("Uncategorized", leaf_label(&[tag("grocery", "")]));
}

// ---------------------------------------------------------------------------
// The budget target's root
//
// Not covered by the Kotlin suite at all — `BudgetPrefs.root` reads a `Context`,
// so it was never JVM-testable. Lifting the rule out of the storage is what
// makes it pinnable, which is the whole argument for this crate in miniature.
// ---------------------------------------------------------------------------

#[test]
fn declared_roots_are_first_segments_in_corpus_order_deduplicated() {
    let tags = vec![
        tag("grocery", "Grocery"),
        tag("grocery/dairy", "Dairy"),
        tag("household", "Household"),
        tag("grocery/bakery", "Bakery"),
        tag("", "Empty"),
    ];
    assert_eq!(vec!["grocery", "household"], declared_roots(&tags));
}

/// The stored choice wins, but only while the corpus still declares it — a
/// stored root can outlive the rule that produced it.
#[test]
fn a_stored_root_wins_only_while_it_is_still_declared() {
    let declared = vec!["grocery".to_string(), "household".to_string()];
    assert_eq!(
        "household",
        resolve_budget_root(Some("household"), &declared)
    );
    // Stored root has since vanished from the corpus -> the fallback, not it.
    assert_eq!("grocery", resolve_budget_root(Some("petcare"), &declared));
}

#[test]
fn the_fallback_root_is_named_rather_than_arbitrary() {
    let declared = vec!["household".to_string(), "grocery".to_string()];
    // "grocery" beats "household" despite being declared second.
    assert_eq!("grocery", resolve_budget_root(None, &declared));
}

#[test]
fn without_grocery_the_first_declared_root_stands_in() {
    let declared = vec!["household".to_string(), "petcare".to_string()];
    assert_eq!("household", resolve_budget_root(None, &declared));
}

/// Never empty, even against a corpus that declares nothing.
#[test]
fn an_empty_corpus_still_resolves_to_something() {
    assert_eq!(FALLBACK_BUDGET_ROOT, resolve_budget_root(None, &[]));
    assert_eq!(
        FALLBACK_BUDGET_ROOT,
        resolve_budget_root(Some("petcare"), &[])
    );
}
