//! What an item cost each time it was bought, from the receipts both apps
//! already store: "what did I pay last time?", and — for an item whose price
//! actually repeats — whether that price has moved.
//!
//! [`price_history`] is one pure function from receipts plus the user's
//! [`ProductLink`]s to item histories. No storage, no clock, no UniFFI: the
//! apps persist the links and cache the result per store revision, the way they
//! already treat `spend-core`.
//!
//! # An item is a merchant plus a printed code, or a merchant plus a cleaned name
//!
//! The merchant is [`HistoryReceipt::merchant_family`] when core recognised
//! one, else the display merchant. Within a merchant:
//!
//! - **A printed product code is the identity**, and the name only a label. OCR
//!   reads one code's name several ways (`ORG MILK-4L`, `ORG MILK/4L`), and
//!   keying on the name split one product's history three ways.
//! - **A line with no code joins the code whose lines print the same name** —
//!   when exactly one code at that merchant does. A code OCR missed on one
//!   receipt should not start a second history.
//! - **Otherwise the cleaned name is the identity** ([`clean_name`]). The parser
//!   folds per-purchase deal text into descriptions (`(2/$5.00)`,
//!   `(1 @ 12.50)`), which would otherwise give every promotion its own item.
//!
//! Codes never join across merchants. An equal code at two stores is a hint for
//! the user, not proof of one product; [`ProductLink`] is the only thing that
//! puts two merchants in one history.
//!
//! # How many units a line paid for
//!
//! A line's price is its amount, and the parser's quantity is `1` whether or not
//! the receipt printed a count — the core leaves `2 @ 6.49` unfolded, and the
//! ledger balances either way. A history cannot leave it: dividing nothing turns
//! two cartons into a doubled egg price. So the units behind an amount come
//! from, in order ([`UnitsBasis`]):
//!
//! 1. a quantity above one that the parser did record;
//! 2. an amount that is an exact multiple (×2 or more) of another price paid for
//!    the same item at the same merchant — 12.98 beside 6.49 is two of them;
//! 3. otherwise one, assumed.
//!
//! An inference survives only if the merchant's prices come out
//! [`Pricing::Steady`] with it. Where prices don't repeat anyway — meat by
//! weight — a divisible amount is a coincidence, not a multi-buy.
//!
//! # Which items have a price worth tracking
//!
//! See [`Pricing`]. Every purchase is listed; only a [`Pricing::Steady`] price
//! is one a screen should compare against. A department line (`MEAT` at $4 one
//! week and $20 the next) is a list of what was paid, never a trend.
//!
//! # Not a unit-price engine
//!
//! Prices are per *counted unit of whatever the line sold*. Nothing here knows
//! a package size or a weight, so a 2 L and a 4 L carton under two codes are two
//! items, and linking them makes one history with incomparable prices. Discount
//! lines are left out, not netted: a price is what the item's own line printed.

use std::collections::{BTreeSet, HashMap};

use spend_core::{parse_iso_date, price_value};
pub use spend_core::{ItemTag, SpendDate};

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// One scanned receipt, projected down to what the history reads.
///
/// A sibling of `spend_core::SpendInput` rather than an extension of it: the
/// spend screens cross the FFI on every render, and widening their projection
/// for one screen's fields would make all of them pay for it.
#[derive(Debug, Clone)]
pub struct HistoryReceipt {
    pub id: String,
    /// `result.merchant` — the display merchant.
    pub merchant: String,
    /// `result.merchantMatch.canonical`. Preferred when present, so a family the
    /// core recognises keys one history however its header was read.
    pub merchant_family: Option<String>,
    /// `result.date`, ISO `YYYY-MM-DD`.
    pub date_iso: Option<String>,
    /// Carried rather than folded into `date_iso` by the caller, so the rule
    /// lives in [`purchase_date`] once instead of in two apps.
    pub date_is_placeholder: bool,
    pub items: Vec<HistoryItem>,
}

/// One printed line.
#[derive(Debug, Clone)]
pub struct HistoryItem {
    pub description: String,
    /// `item.itemNumber`: the merchant's printed product code.
    pub item_number: Option<String>,
    /// The raw printed price — the line's amount, not a unit price.
    pub price: String,
    /// What the parser recorded. `1` is its default, not evidence of one unit.
    pub quantity: i32,
    pub tags: Vec<ItemTag>,
    /// `item.giftCard != nil`. Buying a gift card is not a price for anything.
    pub is_gift_card: bool,
}

/// The user's decision that items are one product: across merchants, or just
/// to give one a better name.
///
/// The app persists these. Passing a different list is the whole merge, split
/// and rename mechanism — nothing here remembers a previous call.
#[derive(Debug, Clone)]
pub struct ProductLink {
    /// Expected unique. Two links sharing an id are one product.
    pub id: String,
    /// The history's title. Blank falls back to the printed name.
    pub name: String,
    /// Keys as [`ItemHistory::members`] reported them. A member with no
    /// purchases is fine; a key claimed by two links goes to the smaller `id`.
    pub members: Vec<ItemKey>,
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// What one item is at one merchant. See the crate docs for how it is chosen.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemKey {
    /// Uppercased, whitespace runs collapsed.
    pub merchant: String,
    pub identity: Identity,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Identity {
    /// The printed product code, trimmed. Case and leading zeros are kept.
    Code(String),
    /// [`clean_name`], uppercased and whitespace-collapsed.
    Name(String),
}

impl ItemKey {
    /// The same key re-folded, so a link read back from storage — or written by
    /// an older build — still matches the keys computed here.
    fn normalized(&self) -> Self {
        Self {
            merchant: fold(&self.merchant),
            identity: match &self.identity {
                Identity::Code(code) => Identity::Code(code.trim().to_owned()),
                Identity::Name(name) => Identity::Name(fold(name)),
            },
        }
    }
}

/// What a history is keyed by: one item, or a user's product.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HistoryKey {
    Item(ItemKey),
    /// [`ProductLink::id`].
    Product(String),
}

// ---------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------

/// Where a purchase's unit count came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitsBasis {
    /// Nothing said otherwise, so one.
    Assumed,
    /// The parser recorded a quantity above one.
    Recorded,
    /// The amount is an exact multiple of another price paid for this item at
    /// this merchant. Worth showing as such ("2 × $6.49"): it is a reading, not
    /// something the receipt printed.
    Inferred,
}

/// One printed line that bought something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Purchase {
    pub receipt_id: String,
    /// Index into the receipt's own items, as the app stores them.
    pub item_index: u32,
    /// The family when there was one, else the display merchant, as supplied.
    pub merchant: String,
    /// As printed, deal text and all.
    pub description: String,
    /// See [`purchase_date`]. `None` sorts after every date.
    pub date: Option<SpendDate>,
    /// Cents. `None` when the price could not be read — still a purchase.
    pub amount: Option<i64>,
    pub units: u32,
    pub basis: UnitsBasis,
    /// `amount / units` in cents, rounded half up.
    pub unit_price: Option<i64>,
}

/// Whether one merchant's prices for an item are a price at all.
///
/// # Steady means at least half the prices repeat
///
/// A packaged item is sold at a handful of prices — regular, sale, a later
/// increase — so most purchases share their unit price with another one. A
/// weighed or department line is a different amount nearly every time.
///
/// On the private corpus (164 receipts, live OCR, 2026-10-01) the merchants'
/// groups with two or more priced purchases split cleanly. The share of
/// purchases repeating another's price was 0–0.12 for every by-weight line;
/// packaged lines started at 0.64. The cut-off barely matters inside that gap:
///
/// | Steady when repeating share ≥ | 1/4 – 2/5 | 1/2 – 3/5 | 2/3 | 3/4 |
/// |---|---|---|---|---|
/// | Steady / Varies groups        | 115 / 51 | 114 / 52 | 113 / 53 | 104 / 62 |
///
/// Half is the round number inside the flat stretch. Two purchases at two
/// different prices are `Varies` — a price rise and a weighed item look the
/// same until a price comes round again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pricing {
    /// At most one readable price: nothing to compare.
    Single,
    /// Prices repeat. Changes between purchases are real price changes.
    Steady,
    /// Prices don't repeat. Show what was paid; claim no trend.
    Varies,
}

/// One merchant's figures for an item. Prices never mix across merchants, even
/// in a linked product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerchantPrices {
    pub merchant: String,
    /// Purchases here, readable price or not.
    pub purchase_count: u32,
    pub pricing: Pricing,
    /// The most recent dated purchase with a readable price.
    pub latest: Option<Purchase>,
    /// Unit prices in cents, over every readable price, dated or not.
    pub lowest: Option<i64>,
    pub highest: Option<i64>,
    /// The most common unit price; between equally common ones, the latest.
    pub typical: Option<i64>,
    /// Total paid over total units — a unit bought six at a time counts six.
    pub average: Option<i64>,
}

/// Everything known about one item, or one linked product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemHistory {
    pub key: HistoryKey,
    /// The link's name, else the most common cleaned description (the latest
    /// among equally common ones).
    pub name: String,
    /// The item keys with purchases here — what a merge or split works on.
    pub members: Vec<ItemKey>,
    /// Newest first; undated last.
    pub purchases: Vec<Purchase>,
    /// The most recent dated purchase with a readable price, at any merchant.
    pub latest: Option<Purchase>,
    pub receipt_count: u32,
    /// Ordered by each merchant's newest purchase.
    pub merchants: Vec<MerchantPrices>,
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// Tag roots for money that moves for some reason other than buying the thing
/// printed. A deposit that names its product files with the product instead,
/// so it never carries this tag.
const NOT_PURCHASES: [&str; 2] = ["discount", "deposit"];

/// The date a receipt's items were bought, or `None`.
///
/// **No fallback to the scan date**, unlike `spend_core::record_date`. A month
/// bucket needs *some* date; "last bought on" must not claim one the receipt
/// never printed.
pub fn purchase_date(receipt: &HistoryReceipt) -> Option<SpendDate> {
    if receipt.date_is_placeholder {
        return None;
    }
    receipt.date_iso.as_deref().and_then(parse_iso_date)
}

/// A description with what varies between purchases of one item taken off:
/// a leading copy of the item's code, a leading `*` marker, and everything from
/// the deal annotation on — the first `(` before an `@`, or the `@` itself.
///
/// `AB - Rice Crackers ((300g)@3.49(1/$1.89))` → `AB - Rice Crackers`;
/// `HOUSE RED (3 @ 12.50)` → `HOUSE RED`; `812 LG EGGS` with code `812` →
/// `LG EGGS`. Brackets without an `@` are part of the name and stay. Never
/// empty: a description that is all annotation is returned whole.
pub fn clean_name<'a>(description: &'a str, code: Option<&str>) -> &'a str {
    let whole = description.trim();
    let mut name = whole;
    if let Some(rest) = code.and_then(|code| name.strip_prefix(code)) {
        if rest.starts_with(char::is_whitespace) {
            name = rest;
        }
    }
    name = name.trim_start_matches(|c: char| c == '*' || c.is_whitespace());
    if let Some(at) = name.find('@') {
        name = &name[..name[..at].find('(').unwrap_or(at)];
    }
    let name = name.trim_end();
    if name.is_empty() {
        whole
    } else {
        name
    }
}

/// Uppercase, whitespace runs collapsed to one space, trimmed.
fn fold(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// A printed price in cents, by `spend_core::price_value`'s reading of it.
fn cents(raw: &str) -> Option<i64> {
    let cents = (price_value(raw)? * 100.0).round();
    // Past 2^53 an f64 no longer holds every integer; no receipt line gets near.
    (cents.abs() < 9.0e15).then_some(cents as i64)
}

/// `amount / units`, rounded half up. Both positive.
fn per_unit(amount: i64, units: u32) -> i64 {
    let (amount, units) = (i128::from(amount), i128::from(units));
    ((2 * amount + units) / (2 * units)) as i64
}

fn is_purchase(item: &HistoryItem, amount: Option<i64>) -> bool {
    let not_purchase = item.tags.iter().any(|tag| {
        let root = tag.path.split('/').next().unwrap_or_default();
        NOT_PURCHASES.contains(&root)
    });
    // Zero or negative is a discount, a return or a parse artefact.
    !item.is_gift_card && !not_purchase && amount.map_or(true, |amount| amount > 0)
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

/// A purchase line before it is grouped.
struct Line<'a> {
    receipt: &'a HistoryReceipt,
    index: u32,
    item: &'a HistoryItem,
    /// Folded — the key's merchant.
    merchant: String,
    date: Option<SpendDate>,
    amount: Option<i64>,
    code: Option<String>,
    /// [`clean_name`], original case — what a title shows.
    label: &'a str,
}

impl Line<'_> {
    fn display_merchant(&self) -> &str {
        family(self.receipt)
            .unwrap_or(&self.receipt.merchant)
            .trim()
    }
}

fn family(receipt: &HistoryReceipt) -> Option<&str> {
    receipt
        .merchant_family
        .as_deref()
        .filter(|family| !family.trim().is_empty())
}

fn lines(receipts: &[HistoryReceipt]) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    for receipt in receipts {
        let merchant = fold(family(receipt).unwrap_or(&receipt.merchant));
        let date = purchase_date(receipt);
        for (index, item) in receipt.items.iter().enumerate() {
            let amount = cents(&item.price);
            if !is_purchase(item, amount) {
                continue;
            }
            let code = item
                .item_number
                .as_deref()
                .map(str::trim)
                .filter(|code| !code.is_empty())
                .map(str::to_owned);
            let label = clean_name(&item.description, code.as_deref());
            lines.push(Line {
                receipt,
                index: index as u32,
                item,
                merchant: merchant.clone(),
                date,
                amount,
                code,
                label,
            });
        }
    }
    lines
}

/// Each line's [`ItemKey`]: its code; else the one code its name is printed
/// with at this merchant; else its name.
fn item_keys(lines: &[Line]) -> Vec<ItemKey> {
    let mut codes_by_name: HashMap<(&str, String), BTreeSet<&str>> = HashMap::new();
    for line in lines {
        if let Some(code) = &line.code {
            codes_by_name
                .entry((&line.merchant, fold(line.label)))
                .or_default()
                .insert(code);
        }
    }
    lines
        .iter()
        .map(|line| {
            let identity = match &line.code {
                Some(code) => Identity::Code(code.clone()),
                None => {
                    let name = fold(line.label);
                    match codes_by_name.get(&(line.merchant.as_str(), name.clone())) {
                        Some(codes) if codes.len() == 1 => {
                            Identity::Code(codes.iter().next().unwrap().to_string())
                        }
                        _ => Identity::Name(name),
                    }
                }
            };
            ItemKey {
                merchant: line.merchant.clone(),
                identity,
            }
        })
        .collect()
}

/// Each linked key → the link that claims it. A key two links both claim goes
/// to the smaller `id`, so the outcome never depends on the order links are
/// stored in.
fn claims(links: &[ProductLink]) -> HashMap<ItemKey, &ProductLink> {
    let mut sorted: Vec<&ProductLink> = links.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut claims = HashMap::new();
    for link in sorted {
        for member in &link.members {
            claims.entry(member.normalized()).or_insert(link);
        }
    }
    claims
}

/// One merchant's units per line and its [`Pricing`]. `lines` are one item's
/// purchases at one merchant.
fn assign_units(lines: &[&Line]) -> (Vec<(u32, UnitsBasis)>, Pricing) {
    let recorded: Vec<(u32, UnitsBasis)> = lines
        .iter()
        .map(|line| match u32::try_from(line.item.quantity) {
            Ok(quantity) if quantity > 1 => (quantity, UnitsBasis::Recorded),
            _ => (1, UnitsBasis::Assumed),
        })
        .collect();
    let unit_prices = |units: &[(u32, UnitsBasis)]| -> Vec<Option<i64>> {
        lines
            .iter()
            .zip(units)
            .map(|(line, &(units, _))| line.amount.map(|amount| per_unit(amount, units)))
            .collect()
    };
    let base = unit_prices(&recorded);

    let mut inferred = recorded.clone();
    for (i, line) in lines.iter().enumerate() {
        let Some(amount) = line.amount else { continue };
        if recorded[i].1 != UnitsBasis::Assumed {
            continue;
        }
        // The smallest other price this amount is a whole multiple of: 45.96
        // beside 11.49 and 22.98 is four, not two.
        let smallest = base
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != i)
            .filter_map(|(_, price)| *price)
            .filter(|&price| price > 0 && price < amount && amount % price == 0)
            .min();
        if let Some(units) = smallest.and_then(|price| u32::try_from(amount / price).ok()) {
            inferred[i] = (units, UnitsBasis::Inferred);
        }
    }

    let pricing = pricing_of(&unit_prices(&inferred));
    if pricing == Pricing::Steady {
        (inferred, pricing)
    } else {
        let pricing = pricing_of(&base);
        (recorded, pricing)
    }
}

/// See [`Pricing`].
fn pricing_of(unit_prices: &[Option<i64>]) -> Pricing {
    let prices: Vec<i64> = unit_prices.iter().flatten().copied().collect();
    if prices.len() < 2 {
        return Pricing::Single;
    }
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for &price in &prices {
        *counts.entry(price).or_default() += 1;
    }
    let repeating: usize = counts.values().filter(|&&n| n >= 2).sum();
    if 2 * repeating >= prices.len() {
        Pricing::Steady
    } else {
        Pricing::Varies
    }
}

/// Newest first, undated last; receipt and line break a tie, so the order never
/// depends on the order receipts were supplied in.
fn newest_first(a: &Purchase, b: &Purchase) -> std::cmp::Ordering {
    (
        a.date.is_none(),
        std::cmp::Reverse(a.date),
        &a.receipt_id,
        a.item_index,
    )
        .cmp(&(
            b.date.is_none(),
            std::cmp::Reverse(b.date),
            &b.receipt_id,
            b.item_index,
        ))
}

/// The first dated purchase with a readable price — the latest, since
/// purchases are kept newest first.
fn latest<'a>(purchases: impl IntoIterator<Item = &'a Purchase>) -> Option<Purchase> {
    purchases
        .into_iter()
        .find(|p| p.date.is_some() && p.unit_price.is_some())
        .cloned()
}

/// The most common value; between equally common ones, the first — `values`
/// arrive newest first, so that is the latest.
fn most_common<T: Eq + std::hash::Hash + Clone>(values: impl Iterator<Item = T>) -> Option<T> {
    let values: Vec<T> = values.collect();
    let mut counts: HashMap<&T, usize> = HashMap::new();
    for value in &values {
        *counts.entry(value).or_default() += 1;
    }
    let top = counts.values().copied().max()?;
    values.iter().find(|value| counts[value] == top).cloned()
}

fn merchant_prices(merchant: String, purchases: &[&Purchase], pricing: Pricing) -> MerchantPrices {
    let priced: Vec<&Purchase> = purchases
        .iter()
        .copied()
        .filter(|p| p.unit_price.is_some())
        .collect();
    let unit_prices = || priced.iter().filter_map(|p| p.unit_price);
    let paid: i128 = priced.iter().filter_map(|p| p.amount).map(i128::from).sum();
    let units: i128 = priced.iter().map(|p| i128::from(p.units)).sum();
    MerchantPrices {
        merchant,
        purchase_count: purchases.len() as u32,
        pricing,
        latest: latest(purchases.iter().copied()),
        lowest: unit_prices().min(),
        highest: unit_prices().max(),
        typical: most_common(unit_prices()),
        average: (units > 0).then(|| ((2 * paid + units) / (2 * units)) as i64),
    }
}

/// One history's lines, each with its own item key, and the link — if any —
/// that made them one.
struct Group<'a> {
    link: Option<&'a ProductLink>,
    lines: Vec<(ItemKey, &'a Line<'a>)>,
}

fn history(key: HistoryKey, Group { link, lines }: Group) -> ItemHistory {
    let members: BTreeSet<ItemKey> = lines.iter().map(|(key, _)| key.clone()).collect();

    // Units are worked out per merchant: a price at one store says nothing about
    // how many were bought at another.
    let mut by_merchant: HashMap<&str, Vec<&Line>> = HashMap::new();
    for (_, line) in &lines {
        by_merchant.entry(&line.merchant).or_default().push(line);
    }
    let mut pricing: HashMap<&str, Pricing> = HashMap::new();
    let mut purchases = Vec::with_capacity(lines.len());
    for (merchant, group) in &by_merchant {
        let (units, merchant_pricing) = assign_units(group);
        pricing.insert(merchant, merchant_pricing);
        for (line, (units, basis)) in group.iter().zip(units) {
            purchases.push((
                line.merchant.as_str(),
                line.label,
                Purchase {
                    receipt_id: line.receipt.id.clone(),
                    item_index: line.index,
                    merchant: line.display_merchant().to_owned(),
                    description: line.item.description.clone(),
                    date: line.date,
                    amount: line.amount,
                    units,
                    basis,
                    unit_price: line.amount.map(|amount| per_unit(amount, units)),
                },
            ));
        }
    }
    purchases.sort_by(|(_, _, a), (_, _, b)| newest_first(a, b));

    let mut merchants: Vec<MerchantPrices> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (merchant, _, purchase) in &purchases {
        if seen.insert(merchant) {
            let here: Vec<&Purchase> = purchases
                .iter()
                .filter(|(m, _, _)| m == merchant)
                .map(|(_, _, p)| p)
                .collect();
            merchants.push(merchant_prices(
                purchase.merchant.clone(),
                &here,
                pricing[merchant],
            ));
        }
    }

    let name = link
        .map(|link| link.name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| most_common(purchases.iter().map(|(_, label, _)| *label)).map(str::to_owned))
        .unwrap_or_default();
    let purchases: Vec<Purchase> = purchases.into_iter().map(|(_, _, p)| p).collect();
    let receipt_count = purchases
        .iter()
        .map(|p| &p.receipt_id)
        .collect::<BTreeSet<_>>()
        .len() as u32;

    ItemHistory {
        key,
        name,
        members: members.into_iter().collect(),
        latest: latest(&purchases),
        purchases,
        receipt_count,
        merchants,
    }
}

/// Every item bought across `receipts`, grouped as the crate docs describe,
/// most recently bought first.
///
/// Total: no input makes this fail. Lines that are not purchases — discounts,
/// deposits, gift cards, zero or negative amounts — are left out; a line whose
/// price can't be read is kept, without a price.
pub fn price_history(receipts: &[HistoryReceipt], links: &[ProductLink]) -> Vec<ItemHistory> {
    let lines = lines(receipts);
    let keys = item_keys(&lines);
    let claims = claims(links);

    let mut groups: HashMap<HistoryKey, Group> = HashMap::new();
    for (line, key) in lines.iter().zip(keys) {
        let link = claims.get(&key).copied();
        let history_key = match link {
            Some(link) => HistoryKey::Product(link.id.clone()),
            None => HistoryKey::Item(key.clone()),
        };
        let group = groups.entry(history_key).or_insert(Group {
            link,
            lines: Vec::new(),
        });
        group.lines.push((key, line));
    }

    let mut histories: Vec<ItemHistory> = groups
        .into_iter()
        .map(|(key, group)| history(key, group))
        .collect();
    histories.sort_by(|a, b| {
        let newest = |h: &ItemHistory| h.purchases.first().and_then(|p| p.date);
        (newest(a).is_none(), std::cmp::Reverse(newest(a)))
            .cmp(&(newest(b).is_none(), std::cmp::Reverse(newest(b))))
            .then_with(|| b.purchases.len().cmp(&a.purchases.len()))
            .then_with(|| a.key.cmp(&b.key))
    });
    histories
}

/// Whether `query` appears in the history's name, a member's merchant, code or
/// name, or any purchase's printed description — case and spacing ignored. An
/// empty query matches everything. A substring test, not fuzzy matching.
pub fn matches(history: &ItemHistory, query: &str) -> bool {
    let query = fold(query);
    let hit = |text: &str| fold(text).contains(&query);
    hit(&history.name)
        || history.members.iter().any(|key| {
            hit(&key.merchant)
                || match &key.identity {
                    Identity::Code(code) => hit(code),
                    Identity::Name(name) => hit(name),
                }
        })
        || history
            .purchases
            .iter()
            .any(|p| hit(&p.description) || hit(&p.merchant))
}
