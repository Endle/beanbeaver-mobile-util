//! The UniFFI seam over [`spend_core`], and the single library both phone apps
//! link.
//!
//! # This library carries two namespaces
//!
//! It depends on `bb-receipt-ffi` purely so that crate's scaffolding lands in
//! the same artifact. The result is one `libbb_mobile_ffi.{so,a,dylib}` exposing
//! **both** `bb_mobile_ffi` (this crate) and `bb_receipt_ffi` (the parse core),
//! so each app pins only this repo and runs one codegen step:
//!
//! ```text
//! uniffi-bindgen generate --library libbb_mobile_ffi.so --language kotlin
//!   -> uniffi/bb_mobile_ffi/…   and   uniffi/bb_receipt_ffi/…
//! ```
//!
//! Two things about that arrangement are load-bearing and easy to undo by
//! accident. Both were established by measurement, not by reading docs:
//!
//! 1. **The `use bb_receipt_ffi as _;` below is not decoration.** A dependency
//!    that is never referenced does not get linked into a `cdylib`, and uniffi's
//!    scaffolding is `#[no_mangle]` statics with nothing referencing them. Drop
//!    that line and the library builds, shrinks from ~59 MB to ~700 KB, and
//!    bindgen silently emits **one** namespace — no error anywhere. CI asserts
//!    both namespaces are present for exactly this reason.
//!
//! 2. **Every type here is prefixed `Spend`.** In Swift both namespaces are
//!    generated into one module, so a type name shared with `bb-receipt-ffi`
//!    is a redeclaration error. Core already exports `ItemTag`, `ReceiptItem`,
//!    `Phase`, `ScanTimings` and ~20 more; it exports nothing starting `Spend`.
//!    Adding an unprefixed type here breaks the iOS build and only the iOS
//!    build.
//!
//! No `uniffi.toml` and no `cdylib_name` are needed: in `--library` mode uniffi
//! stamps the scanned artifact's name into every namespace it emits.
//!
//! # Why the types are mirrored rather than reused
//!
//! [`spend_core`] has no dependencies at all — that is what lets it be tested
//! anywhere, and it is worth keeping. So the records below mirror its types and
//! convert at the boundary. The duplication is mechanical and the compiler
//! checks it; the alternative is a `uniffi` feature reaching down into a crate
//! whose whole value is not having one.

// Load-bearing. See item 1 in the module docs — without this the parse core's
// namespace silently vanishes from the built library.
use bb_receipt_ffi as _;

use spend_core as core;

uniffi::setup_scaffolding!();

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// A calendar date, already resolved in the caller's local timezone.
///
/// The app resolves this, not Rust: turning an instant into a local date needs a
/// timezone database *and* the offset in force at that instant, which each OS
/// already has (`Instant.atZone(systemDefault())` / `Calendar.current`).
#[derive(uniffi::Record)]
pub struct SpendDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

/// One classification tag. Mirrors core's `ItemTag`, renamed to keep the two
/// namespaces from colliding in Swift's single module.
#[derive(uniffi::Record)]
pub struct SpendTag {
    pub path: String,
    pub display: String,
}

/// One line item, projected down to what the arithmetic reads.
#[derive(uniffi::Record)]
pub struct SpendItem {
    pub description: String,
    pub price: String,
    pub tags: Vec<SpendTag>,
}

/// One scanned receipt, projected down to what the arithmetic reads.
///
/// Deliberately **not** the app's whole spend record: that carries a full parse
/// result including `rawText` and `beancount`, and both apps recompute the
/// summary on every render, so passing the full thing would copy every OCR dump
/// per frame. Mapping a record into this is about ten lines per platform.
#[derive(uniffi::Record)]
pub struct SpendInput {
    pub id: String,
    pub date_iso: Option<String>,
    /// Carried explicitly so "a placeholder date doesn't count" stays one rule
    /// in one place, rather than two per-platform mappings that can drift.
    pub date_is_placeholder: bool,
    pub scanned_on: SpendDate,
    pub is_excluded: bool,
    pub total: String,
    pub tax: Option<String>,
    pub items: Vec<SpendItem>,
}

/// What a category is selected by — a whole top-level group, or one leaf in it.
///
/// A root is selected by its raw tag id, not its display label: matching on the
/// label would drop every item in the group that didn't carry the root tag.
#[derive(uniffi::Enum)]
pub enum SpendCategory {
    Root { id: String },
    Leaf { label: String },
}

// ---------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------

/// One leaf category — the most specific label the classifier reached.
#[derive(uniffi::Record)]
pub struct SpendLeaf {
    pub label: String,
    pub amount: f64,
    pub item_count: u32,
}

/// One top-level category and the leaves beneath it, largest first.
#[derive(uniffi::Record)]
pub struct SpendRootGroup {
    pub id: String,
    pub label: String,
    pub amount: f64,
    pub item_count: u32,
    pub leaves: Vec<SpendLeaf>,
}

/// A month's arithmetic. Every figure comes from the items, not the receipt
/// total; `receipt_total` is carried solely to reconcile against.
#[derive(uniffi::Record)]
pub struct SpendMonth {
    pub id: String,
    /// English. The caller has `year`/`month` if it would rather localize.
    pub label: String,
    pub year: i32,
    pub month: u32,
    pub tracked: f64,
    pub items_total: f64,
    pub roots: Vec<SpendRootGroup>,
    pub tax: f64,
    pub receipt_total: f64,
    pub receipt_count: u32,
    pub excluded_count: u32,
    pub unreadable_price_count: u32,
    /// Includes excluded records — the Receipts list still shows those.
    pub record_ids: Vec<String>,
    pub max_leaf_amount: f64,
    pub unaccounted: Option<f64>,
}

/// One line item, identified well enough for the caller to find its own object.
#[derive(uniffi::Record)]
pub struct SpendItemEntry {
    pub id: String,
    pub record_id: String,
    pub item_index: u32,
    pub description: String,
    pub price: String,
    pub amount: f64,
}

/// One receipt's contribution to a category.
#[derive(uniffi::Record)]
pub struct SpendReceiptGroup {
    pub record_id: String,
    pub entries: Vec<SpendItemEntry>,
    pub amount: f64,
    /// Context only — never what the group spends.
    pub receipt_total: Option<f64>,
}

/// A half-open span of days, `[start, end)` — a receipt dated `end` belongs to
/// the next span.
#[derive(uniffi::Record)]
pub struct SpendDateRange {
    pub start: SpendDate,
    pub end: SpendDate,
}

/// One bucket of a trend series, carrying the span it covers.
///
/// The span travels with the amount because the newest bucket is normally a
/// *partial* week — the one containing today — and only the view can say so.
#[derive(uniffi::Record)]
pub struct SpendTrendPoint {
    pub range: SpendDateRange,
    pub amount: f64,
}

/// The weekly series behind the home chart and the scoped week-over-week card.
///
/// Every figure is rounded to whole cents, so the value that crosses this seam
/// is the value drawn: two identical weeks give a `delta` of exactly `0.0`, and
/// a view can say "same as last week" with a plain comparison.
#[derive(uniffi::Record)]
pub struct SpendTrend {
    /// Oldest first. The last is the week containing today, and is partial for
    /// six days out of seven.
    pub points: Vec<SpendTrendPoint>,
    /// The mean of `points` — the chart's dashed reference line.
    pub mean: f64,
    /// `week_to_date` minus `previous_week_to_date` — "↑ $15.20 vs last week".
    ///
    /// **Not the newest bucket minus the one before it.** The newest bucket is a
    /// partial week six days out of seven, so that comparison reads as a steep
    /// decline every Monday. This compares the week so far against the same span
    /// of the previous week.
    pub delta: f64,
    /// This week from its first day through today inclusive.
    pub week_to_date: f64,
    /// The same span shifted back seven days.
    pub previous_week_to_date: f64,
    /// The span `week_to_date` covers; `previous_week_to_date` covers it shifted
    /// back seven days.
    pub week_to_date_range: SpendDateRange,
    /// The trailing window ending today inclusive — the "last 30 days" figure.
    pub rolling: f64,
    pub rolling_range: SpendDateRange,
}

// ---------------------------------------------------------------------------
// Conversions
//
// Mechanical and compiler-checked. Inbound types convert into core's; outbound
// types convert from core's.
// ---------------------------------------------------------------------------

impl From<SpendDate> for core::SpendDate {
    fn from(v: SpendDate) -> Self {
        core::SpendDate::new(v.year, v.month, v.day)
    }
}

impl From<SpendTag> for core::ItemTag {
    fn from(v: SpendTag) -> Self {
        core::ItemTag {
            path: v.path,
            display: v.display,
        }
    }
}

impl From<SpendItem> for core::SpendItem {
    fn from(v: SpendItem) -> Self {
        core::SpendItem {
            description: v.description,
            price: v.price,
            tags: v.tags.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SpendInput> for core::SpendInput {
    fn from(v: SpendInput) -> Self {
        core::SpendInput {
            id: v.id,
            date_iso: v.date_iso,
            date_is_placeholder: v.date_is_placeholder,
            scanned_on: v.scanned_on.into(),
            is_excluded: v.is_excluded,
            total: v.total,
            tax: v.tax,
            items: v.items.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SpendCategory> for core::Category {
    fn from(v: SpendCategory) -> Self {
        match v {
            SpendCategory::Root { id } => core::Category::Root(id),
            SpendCategory::Leaf { label } => core::Category::Leaf(label),
        }
    }
}

impl From<core::Leaf> for SpendLeaf {
    fn from(v: core::Leaf) -> Self {
        SpendLeaf {
            label: v.label,
            amount: v.amount,
            item_count: v.item_count,
        }
    }
}

impl From<core::RootGroup> for SpendRootGroup {
    fn from(v: core::RootGroup) -> Self {
        SpendRootGroup {
            id: v.id,
            label: v.label,
            amount: v.amount,
            item_count: v.item_count,
            leaves: v.leaves.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<core::Month> for SpendMonth {
    fn from(v: core::Month) -> Self {
        SpendMonth {
            id: v.id,
            label: v.label,
            year: v.year,
            month: v.month,
            tracked: v.tracked,
            items_total: v.items_total,
            roots: v.roots.into_iter().map(Into::into).collect(),
            tax: v.tax,
            receipt_total: v.receipt_total,
            receipt_count: v.receipt_count,
            excluded_count: v.excluded_count,
            unreadable_price_count: v.unreadable_price_count,
            record_ids: v.record_ids,
            max_leaf_amount: v.max_leaf_amount,
            unaccounted: v.unaccounted,
        }
    }
}

impl From<core::ItemEntry> for SpendItemEntry {
    fn from(v: core::ItemEntry) -> Self {
        SpendItemEntry {
            id: v.id,
            record_id: v.record_id,
            item_index: v.item_index,
            description: v.description,
            price: v.price,
            amount: v.amount,
        }
    }
}

impl From<core::ReceiptGroup> for SpendReceiptGroup {
    fn from(v: core::ReceiptGroup) -> Self {
        SpendReceiptGroup {
            record_id: v.record_id,
            entries: v.entries.into_iter().map(Into::into).collect(),
            amount: v.amount,
            receipt_total: v.receipt_total,
        }
    }
}

impl From<core::DateRange> for SpendDateRange {
    fn from(v: core::DateRange) -> Self {
        SpendDateRange {
            start: v.start.into(),
            end: v.end.into(),
        }
    }
}

impl From<core::SpendDate> for SpendDate {
    fn from(v: core::SpendDate) -> Self {
        SpendDate {
            year: v.year,
            month: v.month,
            day: v.day,
        }
    }
}

impl From<core::TrendPoint> for SpendTrendPoint {
    fn from(v: core::TrendPoint) -> Self {
        SpendTrendPoint {
            range: v.range.into(),
            amount: v.amount,
        }
    }
}

impl From<core::Trend> for SpendTrend {
    fn from(v: core::Trend) -> Self {
        SpendTrend {
            points: v.points.into_iter().map(Into::into).collect(),
            mean: v.mean,
            delta: v.delta,
            week_to_date: v.week_to_date,
            previous_week_to_date: v.previous_week_to_date,
            week_to_date_range: v.week_to_date_range.into(),
            rolling: v.rolling,
            rolling_range: v.rolling_range.into(),
        }
    }
}

fn to_core_records(records: Vec<SpendInput>) -> Vec<core::SpendInput> {
    records.into_iter().map(Into::into).collect()
}

// ---------------------------------------------------------------------------
// Exported functions
//
// Prefixed `spend_` so the call site reads unambiguously next to the parse
// core's own functions, which share a module in Swift and a wildcard import in
// Kotlin.
// ---------------------------------------------------------------------------

/// The calendar month a record belongs to: its own receipt date, unless that is
/// missing, a placeholder or unparseable, in which case its scan date steps in.
#[uniffi::export]
pub fn spend_month_id(record: SpendInput) -> String {
    core::month_id(&record.into())
}

/// Every month with at least one record, newest first.
#[uniffi::export]
pub fn spend_month_ids(records: Vec<SpendInput>) -> Vec<String> {
    core::month_ids(&to_core_records(records))
}

/// The current calendar month's id. The caller supplies "today".
#[uniffi::export]
pub fn spend_current_month_id(today: SpendDate) -> String {
    core::current_month_id(today.into())
}

/// The month a screen opens on: the newest one with receipts, falling back to
/// the current calendar month. Deliberately not "the current month" — a screen
/// opening on a $0.00 month because the last receipt was in September shows
/// nothing and looks broken.
#[uniffi::export]
pub fn spend_default_month_id(records: Vec<SpendInput>, today: SpendDate) -> String {
    core::default_month_id(&to_core_records(records), today.into())
}

/// `"2026-07"` → `"July 2026"`, or the id unchanged if it isn't a month id.
#[uniffi::export]
pub fn spend_month_label(id: String) -> String {
    core::month_label(&id)
}

/// Everything a month adds up to.
#[uniffi::export]
pub fn spend_month(id: String, records: Vec<SpendInput>) -> SpendMonth {
    core::month(&id, &to_core_records(records)).into()
}

/// Every item under `category`, in record order and printed order within a
/// receipt. Excluded receipts are left out, matching every other figure.
#[uniffi::export]
pub fn spend_items(category: SpendCategory, records: Vec<SpendInput>) -> Vec<SpendItemEntry> {
    core::items(&category.into(), &to_core_records(records))
        .into_iter()
        .map(Into::into)
        .collect()
}

/// [`spend_items`], grouped by the receipt each item was printed on.
#[uniffi::export]
pub fn spend_receipt_groups(
    category: SpendCategory,
    records: Vec<SpendInput>,
) -> Vec<SpendReceiptGroup> {
    core::receipt_groups(&category.into(), &to_core_records(records))
        .into_iter()
        .map(Into::into)
        .collect()
}

/// A printed price as a number, or `None` when it can't be read.
#[uniffi::export]
pub fn spend_price_value(raw: String) -> Option<f64> {
    core::price_value(&raw)
}

/// The item's display leaf — the last tag with authored wording.
#[uniffi::export]
pub fn spend_leaf_label(tags: Vec<SpendTag>) -> String {
    let tags: Vec<core::ItemTag> = tags.into_iter().map(Into::into).collect();
    core::leaf_label(&tags)
}

/// Root tags the rule corpus declares: first path segment only, in corpus
/// order, de-duplicated. What the budget root picker offers.
#[uniffi::export]
pub fn spend_declared_roots(tags: Vec<SpendTag>) -> Vec<String> {
    let tags: Vec<core::ItemTag> = tags.into_iter().map(Into::into).collect();
    core::declared_roots(&tags)
}

/// The budget target's root: the stored choice if the corpus still declares it,
/// else `"grocery"` if that is declared, else whatever is declared first.
///
/// Only the rule is here; where the choice is stored stays platform.
#[uniffi::export]
pub fn spend_resolve_budget_root(stored: Option<String>, declared: Vec<String>) -> String {
    core::resolve_budget_root(stored.as_deref(), &declared)
}

/// The weekly trend for `scope` — the home chart, and the Spending screen's
/// scoped week-over-week card, in one call.
///
/// `scope` of `None` means all spending, and is **items plus tax** so it agrees
/// with `SpendMonth::tracked`, the headline the home chart sits under. A
/// category scope is items alone, because tax is not attributable to one.
///
/// `first_weekday` is `1 = Sunday … 7 = Saturday` — ICU's numbering, which
/// `Calendar.current.firstWeekday` gives directly. **Kotlin's
/// `WeekFields.firstDayOfWeek` is a `DayOfWeek` (`MONDAY = 1 … SUNDAY = 7`) and
/// must be converted**, or the two apps draw the same receipts in different
/// weeks. Out-of-range values fall back to Sunday.
///
/// One call rather than four: both apps rebuild the summary on every render and
/// cross this seam with the whole record list each time, so the number of
/// crossings per frame is the thing worth keeping down.
#[uniffi::export]
pub fn spend_trend(
    records: Vec<SpendInput>,
    scope: Option<SpendCategory>,
    today: SpendDate,
    first_weekday: u32,
    weeks: u32,
    rolling_days: u32,
) -> SpendTrend {
    let scope = scope.map(core::Category::from);
    core::trend(
        &to_core_records(records),
        scope.as_ref(),
        today.into(),
        first_weekday,
        weeks,
        rolling_days,
    )
    .into()
}

/// Sentinel root for items the classifier left untagged, exposed so a caller can
/// recognise the group rather than string-matching `"uncategorized"`.
#[uniffi::export]
pub fn spend_uncategorized_root() -> String {
    core::UNCATEGORIZED_ROOT.to_string()
}

/// The budget root used when nothing is stored and nothing better is declared.
#[uniffi::export]
pub fn spend_fallback_budget_root() -> String {
    core::FALLBACK_BUDGET_ROOT.to_string()
}
