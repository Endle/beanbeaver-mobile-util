//! The specification for the weekly trend — the series behind the home card's
//! chart and the Spending screen's scoped week-over-week card.
//!
//! New surface, so unlike `spend_summary.rs` there is no Kotlin twin to port
//! from. These assert the behaviour the two apps must share, which is most of
//! the reason the arithmetic is here rather than in a view: the calendar edges
//! below (leap day, year boundary, locale week start) are exactly where two
//! hand-written implementations would drift.

use spend_core::{
    bucketed, record_date, trend, Category, DateRange, ItemTag, SpendDate, SpendInput, SpendItem,
};

// ---------------------------------------------------------------------------
// Fixtures — same shape as spend_summary.rs, so the two read alike.
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

/// A receipt dated `iso` carrying one grocery/dairy item at `price`.
fn dairy_receipt(id: &str, iso: &str, price: &str) -> SpendInput {
    SpendInput {
        id: id.into(),
        date_iso: Some(iso.into()),
        date_is_placeholder: false,
        scanned_on: SpendDate::new(2026, 1, 1),
        is_excluded: false,
        total: price.into(),
        tax: None,
        items: vec![item("Milk", price, vec![grocery(), dairy()])],
    }
}

fn day(y: i32, m: u32, d: u32) -> SpendDate {
    SpendDate::new(y, m, d)
}

fn range(from: SpendDate, to: SpendDate) -> DateRange {
    DateRange {
        start: from,
        end: to,
    }
}

const SUNDAY: u32 = 1;
const MONDAY: u32 = 2;

fn amounts(t: &spend_core::Trend) -> Vec<f64> {
    t.points.iter().map(|p| p.amount).collect()
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 0.001, "expected {b}, got {a}");
}

// ---------------------------------------------------------------------------
// Bucketing
// ---------------------------------------------------------------------------

#[test]
fn a_receipt_lands_in_the_range_containing_its_date() {
    let records = vec![
        dairy_receipt("a", "2026-03-02", "10.00"),
        dairy_receipt("b", "2026-03-09", "20.00"),
    ];
    let totals = bucketed(
        &records,
        None,
        &[
            range(day(2026, 3, 2), day(2026, 3, 9)),
            range(day(2026, 3, 9), day(2026, 3, 16)),
        ],
    );
    assert_eq!(totals, vec![10.00, 20.00]);
}

#[test]
fn ranges_are_half_open_so_the_end_date_belongs_to_the_next_bucket() {
    // The boundary receipt is dated exactly on the seam. It must land once, in
    // the later bucket — a closed range would double-count it.
    let records = vec![dairy_receipt("seam", "2026-03-09", "10.00")];
    let totals = bucketed(
        &records,
        None,
        &[
            range(day(2026, 3, 2), day(2026, 3, 9)),
            range(day(2026, 3, 9), day(2026, 3, 16)),
        ],
    );
    assert_eq!(totals, vec![0.00, 10.00]);
}

#[test]
fn a_receipt_outside_every_range_lands_nowhere() {
    let records = vec![dairy_receipt("old", "2025-01-01", "99.00")];
    let totals = bucketed(&records, None, &[range(day(2026, 3, 2), day(2026, 3, 9))]);
    assert_eq!(totals, vec![0.00]);
}

#[test]
fn excluded_receipts_are_left_out_matching_every_other_figure() {
    let mut excluded = dairy_receipt("x", "2026-03-03", "50.00");
    excluded.is_excluded = true;
    let records = vec![dairy_receipt("a", "2026-03-03", "10.00"), excluded];
    let totals = bucketed(&records, None, &[range(day(2026, 3, 2), day(2026, 3, 9))]);
    assert_eq!(totals, vec![10.00]);
}

#[test]
fn a_placeholder_date_falls_back_to_the_scan_date() {
    // Same rule as `month_id`, at day resolution: a receipt must not land in one
    // month by one rule and one week by another.
    let mut record = dairy_receipt("p", "2026-03-03", "10.00");
    record.date_is_placeholder = true;
    record.scanned_on = day(2026, 3, 11);
    assert_eq!(record_date(&record), day(2026, 3, 11));

    let totals = bucketed(
        &[record],
        None,
        &[
            range(day(2026, 3, 2), day(2026, 3, 9)),
            range(day(2026, 3, 9), day(2026, 3, 16)),
        ],
    );
    assert_eq!(totals, vec![0.00, 10.00]);
}

#[test]
fn an_unparseable_date_falls_back_to_the_scan_date() {
    let mut record = dairy_receipt("u", "not-a-date", "10.00");
    record.scanned_on = day(2026, 3, 11);
    assert_eq!(record_date(&record), day(2026, 3, 11));
}

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

#[test]
fn unscoped_is_items_plus_tax_so_it_agrees_with_the_month_headline() {
    // `Month::tracked` is items + tax, and the home chart sits directly under
    // that number. A series that dropped tax would quietly disagree with it.
    let mut record = dairy_receipt("a", "2026-03-03", "10.00");
    record.tax = Some("1.30".into());
    let totals = bucketed(&[record], None, &[range(day(2026, 3, 2), day(2026, 3, 9))]);
    assert_eq!(totals, vec![11.30]);
}

#[test]
fn a_scoped_series_is_items_alone_because_tax_has_no_category() {
    let mut record = dairy_receipt("a", "2026-03-03", "10.00");
    record.tax = Some("1.30".into());
    let scope = Category::Root("grocery".into());
    let totals = bucketed(
        &[record],
        Some(&scope),
        &[range(day(2026, 3, 2), day(2026, 3, 9))],
    );
    assert_eq!(totals, vec![10.00]);
}

#[test]
fn a_scope_selects_only_its_own_items() {
    let record = SpendInput {
        id: "mixed".into(),
        date_iso: Some("2026-03-03".into()),
        date_is_placeholder: false,
        scanned_on: day(2026, 1, 1),
        is_excluded: false,
        total: "30.00".into(),
        tax: None,
        items: vec![
            item("Milk", "10.00", vec![grocery(), dairy()]),
            item("Soap", "20.00", vec![household()]),
        ],
    };
    let week = [range(day(2026, 3, 2), day(2026, 3, 9))];

    let root = Category::Root("grocery".into());
    assert_eq!(
        bucketed(std::slice::from_ref(&record), Some(&root), &week),
        vec![10.00]
    );

    let leaf = Category::Leaf("Dairy".into());
    assert_eq!(
        bucketed(std::slice::from_ref(&record), Some(&leaf), &week),
        vec![10.00]
    );

    assert_eq!(bucketed(&[record], None, &week), vec![30.00]);
}

#[test]
fn an_unreadable_price_counts_as_zero_rather_than_dropping_the_receipt() {
    let mut record = dairy_receipt("a", "2026-03-03", "10.00");
    record.items = vec![
        item("Milk", "N/A", vec![grocery(), dairy()]),
        item("Eggs", "6.99", vec![grocery(), dairy()]),
    ];
    let totals = bucketed(&[record], None, &[range(day(2026, 3, 2), day(2026, 3, 9))]);
    assert_eq!(totals, vec![6.99]);
}

// ---------------------------------------------------------------------------
// Week boundaries
// ---------------------------------------------------------------------------

#[test]
fn the_newest_week_is_the_one_containing_today() {
    // 2026-03-04 is a Wednesday; the Sunday-start week containing it began
    // 2026-03-01 and ends 2026-03-08.
    let t = trend(&[], None, day(2026, 3, 4), SUNDAY, 6, 30);
    let newest = t.points.last().expect("six weeks");
    assert_eq!(newest.range.start, day(2026, 3, 1));
    assert_eq!(newest.range.end, day(2026, 3, 8));
}

#[test]
fn the_first_weekday_moves_the_boundary() {
    // The same Wednesday, weeks starting Monday: 2026-03-02 through 2026-03-09.
    // Kotlin's DayOfWeek numbering differs from ICU's and must be converted by
    // the caller — this is the assertion that would catch it if it weren't.
    let t = trend(&[], None, day(2026, 3, 4), MONDAY, 6, 30);
    let newest = t.points.last().expect("six weeks");
    assert_eq!(newest.range.start, day(2026, 3, 2));
    assert_eq!(newest.range.end, day(2026, 3, 9));
}

#[test]
fn today_on_the_first_weekday_starts_a_fresh_week() {
    // 2026-03-01 is a Sunday, so with Sunday-start weeks it is day one of the
    // newest bucket, not the last day of the previous one.
    let t = trend(&[], None, day(2026, 3, 1), SUNDAY, 6, 30);
    assert_eq!(
        t.points.last().expect("six weeks").range.start,
        day(2026, 3, 1)
    );
}

#[test]
fn weeks_are_returned_oldest_first_and_tile_without_gaps() {
    let t = trend(&[], None, day(2026, 3, 4), SUNDAY, 6, 30);
    assert_eq!(t.points.len(), 6);
    // Each week ends exactly where the next begins: no gap a receipt could fall
    // into, and no overlap it could be counted in twice.
    for pair in t.points.windows(2) {
        assert_eq!(pair[0].range.end, pair[1].range.start);
    }
    assert_eq!(t.points[0].range.start, day(2026, 1, 25));
}

#[test]
fn weeks_cross_a_year_boundary() {
    // 2026-01-06 is a Tuesday, so the newest week starts Sunday 2026-01-04 and
    // the oldest of six starts 35 days earlier — 2025-11-30, back through both
    // the year and the month boundary.
    let t = trend(&[], None, day(2026, 1, 6), SUNDAY, 6, 30);
    assert_eq!(t.points[0].range.start, day(2025, 11, 30));
    assert_eq!(
        t.points.last().expect("six weeks").range.start,
        day(2026, 1, 4)
    );
}

#[test]
fn weeks_cross_a_leap_day() {
    // 2024 is a leap year. The week containing 2024-03-01 (a Friday) starts
    // 2024-02-25, and the one before it starts 2024-02-18 — seven days apart
    // across February 29th.
    let t = trend(&[], None, day(2024, 3, 1), SUNDAY, 2, 30);
    assert_eq!(t.points[0].range.start, day(2024, 2, 18));
    assert_eq!(t.points[1].range.start, day(2024, 2, 25));
    assert_eq!(t.points[1].range.end, day(2024, 3, 3));
}

#[test]
fn an_out_of_range_first_weekday_falls_back_to_sunday() {
    let fallback = trend(&[], None, day(2026, 3, 4), 0, 1, 30);
    let sunday = trend(&[], None, day(2026, 3, 4), SUNDAY, 1, 30);
    assert_eq!(fallback.points[0].range, sunday.points[0].range);
}

// ---------------------------------------------------------------------------
// Derived figures
// ---------------------------------------------------------------------------

#[test]
fn the_delta_compares_the_week_so_far_with_the_same_span_last_week() {
    // Today is Wednesday 2026-03-04, weeks start Sunday. The comparison window
    // is Sun–Wed of each week, not the whole of last week.
    let records = vec![
        dairy_receipt("prev", "2026-02-25", "30.00"), // Wed of last week
        dairy_receipt("now", "2026-03-04", "45.20"),  // Wed of this week
    ];
    let t = trend(&records, None, day(2026, 3, 4), SUNDAY, 6, 30);
    approx(t.week_to_date, 45.20);
    approx(t.previous_week_to_date, 30.00);
    approx(t.delta, 15.20);
    assert_eq!(t.week_to_date_range.start, day(2026, 3, 1));
    assert_eq!(t.week_to_date_range.end, day(2026, 3, 5));
}

#[test]
fn spending_later_in_the_previous_week_is_outside_the_comparison() {
    // This is what separates week-to-date from whole-week: today is Wednesday,
    // and last Friday's $99 is in last week's *bucket* but not in the Sun–Wed
    // span being compared. Comparing whole weeks would call this a $99 fall.
    let records = vec![
        dairy_receipt("mon", "2026-02-23", "10.00"), // inside the span
        dairy_receipt("fri", "2026-02-27", "99.00"), // after it, same week
        dairy_receipt("now", "2026-03-02", "10.00"), // Monday this week
    ];
    let t = trend(&records, None, day(2026, 3, 4), SUNDAY, 6, 30);
    approx(t.previous_week_to_date, 10.00);
    approx(t.delta, 0.00);

    // The chart still shows the whole week, $99 included — the exclusion is the
    // comparison's, not the series'.
    let last_week = &t.points[t.points.len() - 2];
    approx(last_week.amount, 109.00);
}

#[test]
fn on_the_last_day_of_the_week_the_comparison_covers_both_weeks_whole() {
    // Saturday 2026-03-07 with Sunday-start weeks: the span is all seven days,
    // so week-to-date and whole-week agree, which is the only day they do.
    let records = vec![
        dairy_receipt("prev", "2026-02-27", "99.00"),
        dairy_receipt("now", "2026-03-06", "40.00"),
    ];
    let t = trend(&records, None, day(2026, 3, 7), SUNDAY, 6, 30);
    assert_eq!(t.week_to_date_range.start, day(2026, 3, 1));
    assert_eq!(t.week_to_date_range.end, day(2026, 3, 8));
    approx(t.previous_week_to_date, 99.00);
    approx(t.week_to_date, 40.00);
    approx(t.delta, -59.00);
}

#[test]
fn the_delta_does_not_depend_on_how_many_weeks_are_charted() {
    // It is computed from `today`, not from the series, so a caller charting
    // one week and a caller charting six get the same headline.
    let records = vec![
        dairy_receipt("prev", "2026-02-25", "30.00"),
        dairy_receipt("now", "2026-03-04", "45.20"),
    ];
    let six = trend(&records, None, day(2026, 3, 4), SUNDAY, 6, 30);
    let one = trend(&records, None, day(2026, 3, 4), SUNDAY, 1, 30);
    let none = trend(&records, None, day(2026, 3, 4), SUNDAY, 0, 30);
    approx(six.delta, 15.20);
    approx(one.delta, 15.20);
    approx(none.delta, 15.20);
}

#[test]
fn two_identical_weeks_give_a_delta_of_exactly_zero() {
    // The reason every figure is rounded to cents at the boundary: an f64
    // subtraction of two equal sums lands near zero, not on it, and a view
    // comparing against 0.0 would render "↑ $0.00" forever.
    let records = vec![
        dairy_receipt("a1", "2026-02-24", "10.10"),
        dairy_receipt("a2", "2026-02-25", "20.20"),
        dairy_receipt("b1", "2026-03-03", "10.10"),
        dairy_receipt("b2", "2026-03-04", "20.20"),
    ];
    let t = trend(&records, None, day(2026, 3, 4), SUNDAY, 6, 30);
    assert_eq!(t.delta, 0.0);
}

#[test]
fn the_mean_averages_every_requested_week_including_empty_ones() {
    // Six weeks requested, one of them spent: the reference line is $10, not
    // $60. An empty week is a real zero, not a missing sample.
    let records = vec![dairy_receipt("a", "2026-03-04", "60.00")];
    let t = trend(&records, None, day(2026, 3, 4), SUNDAY, 6, 30);
    approx(t.mean, 10.00);
}

#[test]
fn the_rolling_window_ends_today_inclusive() {
    // "The last 30 days" is today and the 29 before it.
    let t = trend(&[], None, day(2026, 3, 4), SUNDAY, 6, 30);
    assert_eq!(t.rolling_range.start, day(2026, 2, 3));
    assert_eq!(t.rolling_range.end, day(2026, 3, 5));
}

#[test]
fn a_receipt_dated_today_is_inside_the_rolling_window() {
    let records = vec![dairy_receipt("today", "2026-03-04", "12.00")];
    let t = trend(&records, None, day(2026, 3, 4), SUNDAY, 6, 30);
    approx(t.rolling, 12.00);
}

#[test]
fn a_receipt_one_day_past_the_window_is_outside_it() {
    let records = vec![
        dairy_receipt("edge", "2026-02-03", "5.00"), // first day in
        dairy_receipt("out", "2026-02-02", "99.00"), // one day early
    ];
    let t = trend(&records, None, day(2026, 3, 4), SUNDAY, 6, 30);
    approx(t.rolling, 5.00);
}

#[test]
fn the_rolling_window_honours_the_scope() {
    let record = SpendInput {
        id: "mixed".into(),
        date_iso: Some("2026-03-03".into()),
        date_is_placeholder: false,
        scanned_on: day(2026, 1, 1),
        is_excluded: false,
        total: "30.00".into(),
        tax: None,
        items: vec![
            item("Milk", "10.00", vec![grocery(), dairy()]),
            item("Soap", "20.00", vec![household()]),
        ],
    };
    let scope = Category::Root("household".into());
    let t = trend(&[record], Some(&scope), day(2026, 3, 4), SUNDAY, 6, 30);
    approx(t.rolling, 20.00);
}

#[test]
fn nothing_scanned_gives_a_flat_series_rather_than_an_empty_one() {
    // The chart still has to draw. Six zeroes is a shape; no points is a crash
    // waiting for the view layer.
    let t = trend(&[], None, day(2026, 3, 4), SUNDAY, 6, 30);
    assert_eq!(amounts(&t), vec![0.0; 6]);
    approx(t.mean, 0.0);
    approx(t.rolling, 0.0);
    assert_eq!(t.delta, 0.0);
}

#[test]
fn zero_weeks_requested_is_empty_rather_than_a_panic() {
    let t = trend(&[], None, day(2026, 3, 4), SUNDAY, 0, 30);
    assert!(t.points.is_empty());
    approx(t.mean, 0.0);
    approx(t.delta, 0.0);
}
