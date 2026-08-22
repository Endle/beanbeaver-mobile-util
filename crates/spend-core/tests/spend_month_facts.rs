//! The specification for [`month_facts`] — the daily average and the
//! previous-month-to-date figure under the home slip's total, plus the two
//! windows the slip names.
//!
//! Every case here is a calendar edge: month lengths that differ, February,
//! a leap February, and the year boundary. That is the whole reason this
//! arithmetic is in Rust — two hand-written view layers agree about August and
//! disagree about March 31st.

use spend_core::{month_facts, DateRange, ItemTag, SpendDate, SpendInput, SpendItem};

// ---------------------------------------------------------------------------
// Fixtures — same shape as spend_trend.rs, so the three read alike.
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

/// A receipt dated `iso` carrying one grocery/dairy item at `price`, no tax.
fn receipt(id: &str, iso: &str, price: &str) -> SpendInput {
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

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 0.001, "expected {b}, got {a}");
}

// ---------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------

#[test]
fn a_month_in_progress_runs_from_the_first_through_today() {
    let facts = month_facts("2026-08", &[], day(2026, 8, 21));
    // Half-open, so the 22nd is the exclusive end of a window covering the 21st.
    assert_eq!(facts.window, range(day(2026, 8, 1), day(2026, 8, 22)));
    assert_eq!(facts.days, 21);
}

#[test]
fn a_finished_month_runs_the_whole_month() {
    let facts = month_facts("2026-07", &[], day(2026, 8, 21));
    assert_eq!(facts.window, range(day(2026, 7, 1), day(2026, 8, 1)));
    assert_eq!(facts.days, 31);
}

#[test]
fn a_month_not_yet_begun_runs_the_whole_month() {
    // A receipt whose date OCR'd into next month puts a month on screen before
    // it starts. Nothing has elapsed, so there is no partial window to take.
    let facts = month_facts("2026-09", &[], day(2026, 8, 21));
    assert_eq!(facts.window, range(day(2026, 9, 1), day(2026, 10, 1)));
    assert_eq!(facts.days, 30);
}

#[test]
fn the_first_of_a_month_is_a_one_day_window() {
    let facts = month_facts("2026-08", &[], day(2026, 8, 1));
    assert_eq!(facts.window, range(day(2026, 8, 1), day(2026, 8, 2)));
    assert_eq!(facts.days, 1);
}

#[test]
fn the_last_day_of_a_month_covers_it_entirely() {
    let facts = month_facts("2026-08", &[], day(2026, 8, 31));
    assert_eq!(facts.window, range(day(2026, 8, 1), day(2026, 9, 1)));
    assert_eq!(facts.days, 31);
}

#[test]
fn february_of_a_leap_year_is_twenty_nine_days() {
    let facts = month_facts("2024-02", &[], day(2024, 3, 15));
    assert_eq!(facts.window, range(day(2024, 2, 1), day(2024, 3, 1)));
    assert_eq!(facts.days, 29);
}

// ---------------------------------------------------------------------------
// The previous window
// ---------------------------------------------------------------------------

#[test]
fn the_previous_window_is_the_same_stretch_one_month_back() {
    let facts = month_facts("2026-08", &[], day(2026, 8, 21));
    assert_eq!(
        facts.previous_window,
        range(day(2026, 7, 1), day(2026, 7, 22))
    );
}

#[test]
fn a_long_window_is_clamped_to_the_previous_months_own_length() {
    // March 1–31 against February, which has 28 days in 2026. Asking for 31
    // would run the comparison three days into March — against the month it is
    // supposed to be comparing.
    let facts = month_facts("2026-03", &[], day(2026, 3, 31));
    assert_eq!(facts.days, 31);
    assert_eq!(
        facts.previous_window,
        range(day(2026, 2, 1), day(2026, 3, 1))
    );
}

#[test]
fn january_compares_against_december_of_the_year_before() {
    let facts = month_facts("2026-01", &[], day(2026, 1, 10));
    assert_eq!(facts.window, range(day(2026, 1, 1), day(2026, 1, 11)));
    assert_eq!(
        facts.previous_window,
        range(day(2025, 12, 1), day(2025, 12, 11))
    );
}

#[test]
fn the_previous_total_is_what_that_stretch_actually_came_to() {
    let records = vec![
        // In the window: July 1–21.
        receipt("a", "2026-07-03", "100.00"),
        receipt("b", "2026-07-21", "25.00"),
        // Out of it: after the 21st, so a July total would count it and this
        // must not.
        receipt("c", "2026-07-30", "500.00"),
        // The month on screen. Not part of the comparison at all.
        receipt("d", "2026-08-05", "60.00"),
    ];
    let facts = month_facts("2026-08", &records, day(2026, 8, 21));
    approx(facts.previous_total, 125.00);
}

#[test]
fn an_excluded_receipt_is_left_out_of_the_previous_total() {
    let mut excluded = receipt("x", "2026-07-04", "80.00");
    excluded.is_excluded = true;
    let records = vec![receipt("a", "2026-07-03", "20.00"), excluded];
    let facts = month_facts("2026-08", &records, day(2026, 8, 21));
    approx(facts.previous_total, 20.00);
}

#[test]
fn the_previous_total_includes_tax_so_it_compares_with_the_month_total() {
    // Unscoped spending is items *plus* tax everywhere else in this crate —
    // `Month::tracked` and `Trend` both — so a figure printed beside the month
    // total has to be measured the same way or the comparison is dishonest.
    let mut taxed = receipt("a", "2026-07-03", "10.00");
    taxed.tax = Some("1.30".into());
    let facts = month_facts("2026-08", &[taxed], day(2026, 8, 21));
    approx(facts.previous_total, 11.30);
}

// ---------------------------------------------------------------------------
// The daily average
// ---------------------------------------------------------------------------

#[test]
fn the_daily_average_is_the_months_total_over_the_days_elapsed() {
    let records = vec![
        receipt("a", "2026-08-02", "100.00"),
        receipt("b", "2026-08-11", "50.00"),
    ];
    let facts = month_facts("2026-08", &records, day(2026, 8, 10));
    // 10 days elapsed, $150.00 tracked — including the receipt dated the 11th,
    // which the month total also counts. See `MonthFacts::daily_average`.
    assert_eq!(facts.days, 10);
    approx(facts.daily_average, 15.00);
}

#[test]
fn a_finished_month_averages_over_its_whole_length() {
    let records = vec![receipt("a", "2026-07-15", "310.00")];
    let facts = month_facts("2026-07", &records, day(2026, 8, 21));
    assert_eq!(facts.days, 31);
    approx(facts.daily_average, 10.00);
}

#[test]
fn the_average_is_rounded_to_whole_cents() {
    // $100 over 3 days is 33.333…; what crosses the FFI is what gets drawn.
    let records = vec![receipt("a", "2026-08-01", "100.00")];
    let facts = month_facts("2026-08", &records, day(2026, 8, 3));
    assert_eq!(facts.daily_average, 33.33);
}

#[test]
fn a_month_with_nothing_in_it_averages_zero() {
    let facts = month_facts("2026-08", &[], day(2026, 8, 21));
    approx(facts.daily_average, 0.0);
    approx(facts.previous_total, 0.0);
}

// ---------------------------------------------------------------------------
// Degenerate input
// ---------------------------------------------------------------------------

#[test]
fn an_id_that_is_not_a_month_yields_zeros_rather_than_a_panic() {
    let records = vec![receipt("a", "2026-08-02", "100.00")];
    let facts = month_facts("not-a-month", &records, day(2026, 8, 21));
    assert_eq!(facts.days, 0);
    approx(facts.daily_average, 0.0);
    approx(facts.previous_total, 0.0);
    assert_eq!(facts.window, range(day(2026, 8, 21), day(2026, 8, 21)));
}
