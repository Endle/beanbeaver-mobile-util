use std::{cmp::Ordering, error::Error, fmt};

/// Conservative normalization for merchant and name; punctuation is significant.
pub(crate) fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// Observed merchant/name/code tuple. Construction canonicalizes case and space
/// in names, trims the code, and maps a blank code to None. Code case, punctuation
/// and leading zeros are retained. Accessors prevent unnormalized map keys.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemKey {
    merchant: String,
    name: String,
    code: Option<String>,
}

impl ItemKey {
    pub fn new(merchant: &str, name: &str, code: Option<&str>) -> Result<Self, HistoryError> {
        let merchant = normalize(merchant);
        let name = normalize(name);
        if merchant.is_empty() || name.is_empty() {
            return Err(HistoryError::EmptyItemKey);
        }
        Ok(Self {
            merchant,
            name,
            code: code
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_owned),
        })
    }

    pub fn merchant(&self) -> &str {
        &self.merchant
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }
}

/// A validated Gregorian purchase date. Unknown/placeholder dates are None on
/// the observation; callers must not substitute scan time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PurchaseDate {
    year: u16,
    month: u8,
    day: u8,
}

impl PurchaseDate {
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, HistoryError> {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => 0,
        };
        if !(1..=9999).contains(&year) || day == 0 || day > days {
            return Err(HistoryError::InvalidDate);
        }
        Ok(Self { year, month, day })
    }

    pub fn year(self) -> u16 {
        self.year
    }
    pub fn month(self) -> u8 {
        self.month
    }
    pub fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for PurchaseDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Stable app IDs, not a product key. Repeated lines on one receipt need distinct
/// item IDs. Reusing an index after insertion/deletion requires app-side remapping.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObservationId {
    pub receipt_id: String,
    pub item_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Product,
    Discount,
    Deposit,
    Return,
    GiftCard,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaxBasis {
    Included,
    Excluded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiscountBasis {
    BeforeDiscounts,
    AfterDiscounts,
}

/// Explicit approval that this observation refers to the specific package in
/// its history group, and that the amount, count and basis have been checked.
/// Must NOT be constructed merely because an OCR item says quantity == 1.
/// Generic labels such as MEAT stay unapproved. Weighted/size-normalized
/// comparisons are outside this counted-package API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComparisonApproval {
    pub units: u32,
    pub tax: TaxBasis,
    pub discounts: DiscountBasis,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub id: ObservationId,
    pub item: ItemKey,
    /// Original line text, retained even when the user supplies a display name.
    pub observed_name: String,
    pub display_name: Option<String>,
    pub date: Option<PurchaseDate>,
    /// Integer currency minor units; None is unreadable, not zero.
    pub amount_minor: Option<i64>,
    /// Currency captured for this receipt, not today's setting. Three ASCII
    /// letters, normalized in series keys; no currency conversion is performed.
    pub currency: Option<String>,
    /// Untrusted/default parser quantity can be shown without approving it.
    pub recorded_quantity: Option<i32>,
    pub kind: LineKind,
    pub comparison: Option<ComparisonApproval>,
}

/// User-confirmed aliases for the SAME packaged product, possibly across stores.
/// Links never rewrite source observations. Every tuple may belong to at most
/// one product; replacing/removing this snapshot is the merge/undo operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductLink {
    pub id: String,
    pub display_name: String,
    pub aliases: Vec<ItemKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HistoryKey {
    Item(ItemKey),
    Product(String),
}

/// Reasons an observation is retained in history but excluded from comparisons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComparisonIssue {
    NotProduct,
    UnknownDate,
    UnknownCurrency,
    UnreadableAmount,
    NegativeAmount,
    Unapproved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub observation: Observation,
    pub comparison_issues: Vec<ComparisonIssue>,
}

/// Exact rational price in currency minor units. No rounding before comparison
/// or averaging. Private fields ensure a nonnegative numerator and positive
/// denominator; the fraction is reduced, so equality and ordering agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitPrice {
    numerator: u64,
    denominator: u64,
}

impl UnitPrice {
    pub(crate) fn new(numerator: u64, denominator: u64) -> Self {
        assert!(denominator > 0);
        let (mut a, mut b) = (numerator, denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Self {
            numerator: numerator / a,
            denominator: denominator / a,
        }
    }
    pub fn numerator(self) -> u64 {
        self.numerator
    }
    pub fn denominator(self) -> u64 {
        self.denominator
    }
}

impl Ord for UnitPrice {
    fn cmp(&self, other: &Self) -> Ordering {
        (u128::from(self.numerator) * u128::from(other.denominator))
            .cmp(&(u128::from(other.numerator) * u128::from(self.denominator)))
    }
}

impl PartialOrd for UnitPrice {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Histories can link stores, but statistics never mix merchants, currencies,
/// tax treatment or discount treatment. Dates stay visible on every point.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SeriesKey {
    pub merchant: String,
    pub currency: String,
    pub tax: TaxBasis,
    pub discounts: DiscountBasis,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricePoint {
    pub source: ObservationId,
    pub date: PurchaseDate,
    pub amount_minor: u64,
    pub units: u32,
    pub unit_price: UnitPrice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceSeries {
    pub key: SeriesKey,
    /// Oldest first, with source ID breaking date ties (not an inferred time).
    pub points: Vec<PricePoint>,
    pub minimum: UnitPrice,
    pub maximum: UnitPrice,
    /// Sum of amounts / sum of confirmed units, not mean of receipt totals.
    pub average: UnitPrice,
    /// All points on the latest date; no arbitrary "last purchase" within a day.
    pub latest: Vec<PricePoint>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemHistory {
    pub key: HistoryKey,
    pub display_name: String,
    /// Includes inactive aliases so searches still find a renamed product.
    pub aliases: Vec<ItemKey>,
    /// Oldest first, undated last; all source observations are retained.
    pub entries: Vec<HistoryEntry>,
    pub receipt_count: usize,
    pub comparisons: Vec<PriceSeries>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryError {
    EmptyItemKey,
    InvalidDate,
    EmptyObservationId,
    DuplicateObservation(ObservationId),
    InvalidCurrency(ObservationId),
    ZeroApprovedUnits(ObservationId),
    InvalidProductLink(String),
    DuplicateProductId(String),
    ConflictingAlias(ItemKey),
    ArithmeticOverflow,
}

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid price history: {self:?}")
    }
}

impl Error for HistoryError {}
