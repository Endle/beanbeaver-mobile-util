//! What a month of scanned receipts adds up to, grouped the way the items were
//! classified. The Rust twin of — and eventual replacement for —
//! `SpendSummary.kt` and `SpendSummary.swift`, which are line-for-line ports of
//! each other today.
//!
//! **Every figure comes from the items, not the receipt total.** A bank feed
//! already knows a Costco run was $148.73; only this app knows it was $54.45
//! grocery, $24.99 household and $58.40 gas. [`Month::receipt_total`] is carried
//! along solely to reconcile against, never to spend from.
//!
//! # This crate has no dependencies, on purpose
//!
//! No clock, no timezone database, no UniFFI, no platform. Everything here is a
//! function from plain data to plain data, so every figure is checkable from a
//! `cargo test` on any host — which is the whole reason the arithmetic is worth
//! lifting out of two apps in the first place.
//!
//! Two consequences shape the API:
//!
//! - **The caller supplies "today"** ([`current_month_id`], [`default_month_id`])
//!   rather than this crate reading a clock. That also makes the behaviour
//!   testable without freezing time.
//! - **The caller supplies the local calendar date a receipt was scanned on**
//!   ([`SpendInput::scanned_on`]), not an epoch timestamp. Turning an instant
//!   into a local date needs a timezone database *and* the offset in force at
//!   that instant, which each OS already has and does correctly
//!   (`Instant.atZone(systemDefault())` / `Calendar.current`). Asking for the
//!   resolved date keeps that where it belongs and keeps this crate empty of
//!   dependencies.
//!
//! # Not `SpendRecord`
//!
//! [`SpendInput`] is a deliberately slim projection. A `SpendRecord` carries a
//! whole parsed `ReceiptResult` — including `rawText` and `beancount`, which are
//! large strings — and both apps recompute the summary fresh on every render, so
//! handing the full record list across an FFI each time would copy every OCR
//! dump per frame. Each app maps its own record into this instead, about ten
//! lines per platform, and gains insulation: a future change to the parse
//! result's shape doesn't ripple into the spend layer.
//!
//! Nothing here holds a reference back to the caller's objects, so the
//! drill-down types identify a receipt by [`SpendInput::id`] and an item by its
//! index. The app resolves those back to whatever it needs to draw.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// A calendar date, already resolved in whatever timezone the caller considers
/// local. Only ever compared and formatted, never arithmetic'd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpendDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl SpendDate {
    pub fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }

    /// `"2026-07"`.
    fn month_id(self) -> String {
        format!("{:04}-{:02}", self.year, self.month)
    }
}

/// One classification tag: a stable `path` plus the authored `display` from the
/// core's tag vocabulary. Mirrors `ItemTag` across the FFI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemTag {
    /// `"grocery"`, `"grocery/dairy"`.
    pub path: String,
    /// Authored wording, used verbatim — never derived by capitalizing `path`.
    pub display: String,
}

impl ItemTag {
    pub fn new(path: impl Into<String>, display: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            display: display.into(),
        }
    }
}

/// One line item, projected down to what the arithmetic actually reads.
#[derive(Debug, Clone)]
pub struct SpendItem {
    pub description: String,
    /// The raw printed price, parsed by [`price_value`]. Kept as text because an
    /// unreadable price is a real state that has to survive to the footer count.
    pub price: String,
    /// Broad → specific, as the classifier emits them.
    pub tags: Vec<ItemTag>,
}

/// One scanned receipt, projected down to what the arithmetic reads.
#[derive(Debug, Clone)]
pub struct SpendInput {
    pub id: String,
    /// `result.date`, ISO `YYYY-MM-DD`.
    pub date_iso: Option<String>,
    /// Carried rather than folded into `date_iso: None` by the caller, so the
    /// rule "a placeholder date doesn't count" lives here and cannot be
    /// implemented two different ways by two apps.
    pub date_is_placeholder: bool,
    /// The local calendar date this receipt was scanned on — the fallback bucket
    /// when the receipt's own date is missing, placeholder or unparseable.
    pub scanned_on: SpendDate,
    /// Kept out of every spend total — returned, business, not mine. Scoped to
    /// the spend figures only; the stored parse and what an export ships are
    /// untouched, which is why an excluded receipt still appears in
    /// [`Month::record_ids`].
    pub is_excluded: bool,
    pub total: String,
    pub tax: Option<String>,
    pub items: Vec<SpendItem>,
}

// ---------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------

/// One leaf category — the most specific label the classifier reached.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf {
    pub label: String,
    pub amount: f64,
    pub item_count: u32,
}

/// One top-level category and the leaves beneath it. The unit the spending
/// screen lists, so a month reads as "where the money went", largest first.
#[derive(Debug, Clone, PartialEq)]
pub struct RootGroup {
    /// The raw root tag (`"grocery"`) — matches the stored budget root.
    pub id: String,
    /// The authored display label (`"Grocery"`), taken from the tag vocabulary
    /// when any item in the group supplied it.
    pub label: String,
    pub amount: f64,
    pub item_count: u32,
    /// Largest first.
    pub leaves: Vec<Leaf>,
}

/// A month's arithmetic.
///
/// `max_leaf_amount` and `unaccounted` are stored fields rather than computed
/// accessors: they are cheap, and Phase 2b hands this record straight across a
/// UniFFI seam, where a record is data and nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct Month {
    /// `"2026-07"`.
    pub id: String,
    /// `"July 2026"` — English; see [`month_label`].
    pub label: String,
    /// The id's parts, so a caller that would rather localize the label itself
    /// doesn't have to re-parse `id` to do it.
    pub year: i32,
    pub month: u32,
    /// The headline: every tracked item plus tax. What the month cost.
    pub tracked: f64,
    /// Items alone — what [`Month::roots`] sums to, and `tracked` minus `tax`.
    pub items_total: f64,
    /// Largest first, "Uncategorized" included so nothing scanned vanishes.
    pub roots: Vec<RootGroup>,
    pub tax: f64,
    /// Sum of each receipt's own total — the reconciliation number.
    pub receipt_total: f64,
    pub receipt_count: u32,
    pub excluded_count: u32,
    pub unreadable_price_count: u32,
    /// Every record in the month **including excluded ones** — the Receipts list
    /// still shows those.
    pub record_ids: Vec<String>,
    /// The largest single leaf anywhere in the month, so every category bar on
    /// screen shares one scale and is actually comparable. Scaling per group
    /// would put each root on its own invisible scale. `0.0` when empty.
    pub max_leaf_amount: f64,
    /// How far `tracked` sits from what the receipts themselves totalled, or
    /// `None` when they agree. Non-`None` is normal rather than alarming: a scan
    /// that reads every item but misses a `-5.00` discount line lands here, as
    /// does one whose total didn't parse.
    pub unaccounted: Option<f64>,
}

impl Month {
    /// The group for `root`, or `None` when the month has no spend under it.
    pub fn group(&self, root: &str) -> Option<&RootGroup> {
        self.roots.iter().find(|g| g.id == root)
    }
}

/// One line item, with the receipt it came from. What a category total is
/// actually made of — tapping "Dairy $19.38" asks *which items*, and a receipt
/// list would answer with whole-receipt totals unrelated to the number tapped.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemEntry {
    /// Stable within a month: the record's id plus the item's index in it. Two
    /// identical lines on one receipt stay distinct rows.
    pub id: String,
    pub record_id: String,
    /// Index into that record's `items`, so the caller can reach its own object.
    pub item_index: u32,
    pub description: String,
    pub price: String,
    /// The parsed price, or `0.0` when it couldn't be read.
    pub amount: f64,
}

/// One receipt's contribution to a category: the items of it that landed under
/// the tapped category.
///
/// The unit the drill-down lists, because a category total is spread over
/// *purchases* — "$8.42 of this Costco run was dairy" is the shape of the
/// answer, and repeating the merchant on every item row buries it.
///
/// Derived from [`items`] rather than accumulated separately: one matching
/// predicate means a group can't disagree with the flat list.
#[derive(Debug, Clone, PartialEq)]
pub struct ReceiptGroup {
    pub record_id: String,
    /// The matching items, in the order they were printed.
    pub entries: Vec<ItemEntry>,
    /// What those items add up to — this receipt's share of the category total.
    pub amount: f64,
    /// The whole receipt's total, or `None` when it didn't parse. Context only.
    pub receipt_total: Option<f64>,
}

/// What a category is selected by — a whole top-level group, or one leaf inside
/// it. Tapping a card's header and tapping a row in it are different questions.
///
/// A root is selected by its **raw tag id**, not its display label: a group's
/// label is whatever authored wording any of its items supplied
/// (`"personalcare"` → `"Personal Care"`), so matching on the label would drop
/// every item in the group that didn't carry the root tag itself. A leaf carries
/// no such id — [`leaf_label`] is the only thing it was ever accumulated under —
/// so it matches on the label it was grouped by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Category {
    Root(String),
    Leaf(String),
}

// ---------------------------------------------------------------------------
// Parsing helpers
//
// These travel with the arithmetic because the arithmetic needs them. The rest
// of each app's formatting layer stays where it is — an FFI hop per formatted
// price would be ceremony for no gain.
// ---------------------------------------------------------------------------

/// A printed price as a number, or `None` when it can't be read.
///
/// Keeps digits, `.` and `-` and parses what's left, so `"$8.42"` → `8.42` and
/// `"-$3.50"` → `-3.5`, while `"N/A"` → `None`. Twin of Kotlin `priceValue` /
/// Swift `PriceFormat.value`.
pub fn price_value(raw: &str) -> Option<f64> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn price_value_opt(raw: Option<&String>) -> Option<f64> {
    raw.and_then(|s| price_value(s))
}

/// The item's display leaf — the last tag with authored wording.
///
/// `display` comes from the core's tag vocabulary and is used verbatim; deriving
/// it by capitalizing the raw path is what once put `energy_drink` on screen as
/// "Energy_drink". Twin of Kotlin `tagDisplay(...).primary`.
pub fn leaf_label(tags: &[ItemTag]) -> String {
    tags.iter()
        .rfind(|t| !t.display.is_empty())
        .map(|t| t.display.clone())
        .unwrap_or_else(|| "Uncategorized".to_string())
}

/// Sentinel root for items the classifier left untagged. Kept as a real group
/// rather than dropped, so the breakdown always reconciles against what was
/// actually scanned.
pub const UNCATEGORIZED_ROOT: &str = "uncategorized";

/// The item's top-level category. The classifier emits tags broad → specific, so
/// the *first* tag carries the root. A path may itself be nested
/// (`"grocery/meat"`), hence the split.
fn root_of(item: &SpendItem) -> String {
    item.tags
        .first()
        .map(|t| t.path.split('/').next().unwrap_or(""))
        .filter(|s| !s.is_empty())
        .unwrap_or(UNCATEGORIZED_ROOT)
        .to_string()
}

/// The authored label for a root, when this item's tag list carries the root tag
/// itself — the vocabulary's own wording beats capitalizing a raw path segment.
fn root_label(item: &SpendItem, root: &str) -> Option<String> {
    item.tags
        .iter()
        .find(|t| t.path == root && !t.display.is_empty())
        .map(|t| t.display.clone())
}

/// Uppercase the first character and leave the rest alone — Kotlin's
/// `replaceFirstChar { it.uppercase() }`. Deliberately *not* a per-word
/// capitalization: root ids are single path segments, so the two agree, and
/// this is the conservative reading of the two.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Month bucketing
// ---------------------------------------------------------------------------

/// Parse a strict ISO `YYYY-MM-DD`. Anything else — a wrong shape, a month of
/// 13, a day of 0 — is `None` and falls back to the scan date, matching
/// `LocalDate.parse` throwing and `DateFormatter` returning nil.
fn parse_iso_date(s: &str) -> Option<SpendDate> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return None;
    }
    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(SpendDate { year, month, day })
}

/// Parse a strict `YYYY-MM` month id.
fn parse_month_id(id: &str) -> Option<(i32, u32)> {
    let b = id.as_bytes();
    if b.len() != 7 || b[4] != b'-' {
        return None;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 4 || c.is_ascii_digit())
    {
        return None;
    }
    let year: i32 = id[0..4].parse().ok()?;
    let month: u32 = id[5..7].parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month))
}

/// The current calendar month's id — what a screen shows before anything has
/// been scanned into it. The caller supplies "today"; this crate has no clock.
pub fn current_month_id(today: SpendDate) -> String {
    today.month_id()
}

/// The calendar month a record belongs to: its own receipt date, unless that is
/// missing, a placeholder or unparseable, in which case the row's own
/// [`SpendInput::scanned_on`] steps in.
///
/// The fallback is the row's own scan date rather than "today", so a bucket
/// can't drift with the clock on a later run.
pub fn month_id(record: &SpendInput) -> String {
    if !record.date_is_placeholder {
        if let Some(parsed) = record.date_iso.as_deref().and_then(parse_iso_date) {
            return parsed.month_id();
        }
    }
    record.scanned_on.month_id()
}

/// Every month with at least one record, newest first.
pub fn month_ids(records: &[SpendInput]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut ids: Vec<String> = Vec::new();
    for record in records {
        let id = month_id(record);
        if seen.insert(id.clone()) {
            ids.push(id);
        }
    }
    ids.sort_by(|a, b| b.cmp(a));
    ids
}

/// The month a screen opens on: the newest one with receipts in it, falling back
/// to the current calendar month when there are none at all.
///
/// Deliberately *not* "the current month": scanning happens in bursts, and a
/// screen that opens on a $0.00 October because the last receipt was in
/// September shows nothing and looks broken.
pub fn default_month_id(records: &[SpendInput], today: SpendDate) -> String {
    month_ids(records)
        .into_iter()
        .next()
        .unwrap_or_else(|| current_month_id(today))
}

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `"2026-07"` → `"July 2026"`, or `id` unchanged if it isn't a month id.
///
/// **English, unconditionally.** The Kotlin twin formats with
/// `Locale.getDefault()` and the Swift twin with the default `DateFormatter`, so
/// on a non-English phone this is a change: the month name stops being
/// localized. That is the consistent reading — every other string in both apps
/// is English, so a French phone today renders an entirely English screen with
/// one French month name on it. A caller that disagrees has [`Month::year`] and
/// [`Month::month`] and can format the label itself.
pub fn month_label(id: &str) -> String {
    match parse_month_id(id) {
        Some((year, month)) => format!("{} {}", MONTH_NAMES[(month - 1) as usize], year),
        None => id.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Drill-down
// ---------------------------------------------------------------------------

fn matches(category: &Category, item: &SpendItem) -> bool {
    match category {
        Category::Root(id) => root_of(item) == *id,
        Category::Leaf(label) => leaf_label(&item.tags) == *label,
    }
}

/// Every item in `records` under `category`, in the order the records were given
/// and within a receipt in the order the items were printed.
///
/// Recomputed from the records rather than stored during accumulation: the
/// substrate is small, and deriving it here means the list and the total can't
/// drift apart. Excluded receipts are left out, matching every other figure on
/// the spending screen.
pub fn items(category: &Category, records: &[SpendInput]) -> Vec<ItemEntry> {
    let mut out = Vec::new();
    for record in records {
        if record.is_excluded {
            continue;
        }
        for (index, item) in record.items.iter().enumerate() {
            if !matches(category, item) {
                continue;
            }
            out.push(ItemEntry {
                id: format!("{}-{}", record.id, index),
                record_id: record.id.clone(),
                item_index: index as u32,
                description: item.description.clone(),
                price: item.price.clone(),
                amount: price_value(&item.price).unwrap_or(0.0),
            });
        }
    }
    out
}

/// [`items`], grouped by the receipt each item was printed on.
///
/// Both orderings are inherited rather than re-sorted: [`items`] walks `records`
/// in the caller's order (newest-first, as the store holds them) and each
/// receipt's items in index order, so accumulating in first-seen order preserves
/// both.
pub fn receipt_groups(category: &Category, records: &[SpendInput]) -> Vec<ReceiptGroup> {
    let totals: HashMap<&str, Option<f64>> = records
        .iter()
        .map(|r| (r.id.as_str(), price_value(&r.total)))
        .collect();

    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Vec<ItemEntry>> = HashMap::new();
    for entry in items(category, records) {
        if !grouped.contains_key(&entry.record_id) {
            order.push(entry.record_id.clone());
        }
        grouped
            .entry(entry.record_id.clone())
            .or_default()
            .push(entry);
    }

    order
        .into_iter()
        .map(|record_id| {
            let entries = grouped.remove(&record_id).unwrap_or_default();
            let amount = entries.iter().map(|e| e.amount).sum();
            let receipt_total = totals.get(record_id.as_str()).copied().flatten();
            ReceiptGroup {
                record_id,
                entries,
                amount,
                receipt_total,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

/// Insertion-ordered accumulation so ties in amount keep a stable order through
/// the largest-first sorts.
struct RootAccumulator {
    label: String,
    amount: f64,
    item_count: u32,
    leaf_order: Vec<String>,
    leaves: HashMap<String, (f64, u32)>,
}

impl RootAccumulator {
    fn new(label: String) -> Self {
        Self {
            label,
            amount: 0.0,
            item_count: 0,
            leaf_order: Vec::new(),
            leaves: HashMap::new(),
        }
    }

    fn add(&mut self, leaf: String, value: f64) {
        self.amount += value;
        self.item_count += 1;
        if !self.leaves.contains_key(&leaf) {
            self.leaf_order.push(leaf.clone());
        }
        let slot = self.leaves.entry(leaf).or_insert((0.0, 0));
        slot.0 += value;
        slot.1 += 1;
    }
}

/// Largest first, ties keeping their existing (insertion) order. `sort_by` is
/// stable, which is what makes that true — the Kotlin and Swift twins rely on
/// the same property of their own sorts.
fn sort_desc_by_amount<T>(v: &mut [T], amount: impl Fn(&T) -> f64) {
    v.sort_by(|a, b| {
        amount(b)
            .partial_cmp(&amount(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Everything a month adds up to.
pub fn month(id: &str, records: &[SpendInput]) -> Month {
    let month_records: Vec<&SpendInput> = records.iter().filter(|r| month_id(r) == id).collect();
    let excluded_count = month_records.iter().filter(|r| r.is_excluded).count() as u32;
    let tracked_records: Vec<&&SpendInput> =
        month_records.iter().filter(|r| !r.is_excluded).collect();

    let mut items_total = 0.0;
    let mut tax = 0.0;
    let mut receipt_total = 0.0;
    let mut unreadable_price_count = 0u32;
    let mut root_order: Vec<String> = Vec::new();
    let mut root_totals: HashMap<String, RootAccumulator> = HashMap::new();

    for record in &tracked_records {
        receipt_total += price_value(&record.total).unwrap_or(0.0);
        if let Some(t) = price_value_opt(record.tax.as_ref()) {
            tax += t;
        }
        for item in &record.items {
            // An unreadable price is counted and carried at zero rather than
            // dropped: the item still happened, and the footer says how many
            // couldn't be read.
            let parsed = price_value(&item.price);
            if parsed.is_none() {
                unreadable_price_count += 1;
            }
            let amount = parsed.unwrap_or(0.0);
            items_total += amount;

            let root_id = root_of(item);
            if !root_totals.contains_key(&root_id) {
                root_order.push(root_id.clone());
                let default_label = if root_id == UNCATEGORIZED_ROOT {
                    "Uncategorized".to_string()
                } else {
                    capitalize_first(&root_id)
                };
                root_totals.insert(root_id.clone(), RootAccumulator::new(default_label));
            }
            let acc = root_totals.get_mut(&root_id).expect("just inserted");
            // Last item carrying the root tag wins, matching both twins.
            if let Some(authored) = root_label(item, &root_id) {
                acc.label = authored;
            }
            acc.add(leaf_label(&item.tags), amount);
        }
    }

    let mut roots: Vec<RootGroup> = root_order
        .into_iter()
        .map(|root_id| {
            let acc = root_totals.remove(&root_id).expect("ordered key exists");
            let RootAccumulator {
                label,
                amount,
                item_count,
                leaf_order,
                mut leaves,
            } = acc;
            let mut leaf_list: Vec<Leaf> = leaf_order
                .into_iter()
                .map(|label| {
                    let (amount, item_count) = leaves.remove(&label).expect("ordered key exists");
                    Leaf {
                        label,
                        amount,
                        item_count,
                    }
                })
                .collect();
            sort_desc_by_amount(&mut leaf_list, |l| l.amount);
            RootGroup {
                id: root_id,
                label,
                amount,
                item_count,
                leaves: leaf_list,
            }
        })
        .collect();
    sort_desc_by_amount(&mut roots, |r| r.amount);

    let tracked = items_total + tax;
    let max_leaf_amount = roots
        .iter()
        .flat_map(|r| r.leaves.iter())
        .map(|l| l.amount)
        .fold(0.0_f64, f64::max);
    let gap = tracked - receipt_total;
    let unaccounted = if gap.abs() >= 0.01 { Some(gap) } else { None };
    let (year, month_num) = parse_month_id(id).unwrap_or((0, 1));

    Month {
        id: id.to_string(),
        label: month_label(id),
        year,
        month: month_num,
        tracked,
        items_total,
        roots,
        tax,
        receipt_total,
        receipt_count: tracked_records.len() as u32,
        excluded_count,
        unreadable_price_count,
        record_ids: month_records.iter().map(|r| r.id.clone()).collect(),
        max_leaf_amount,
        unaccounted,
    }
}
