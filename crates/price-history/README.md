# price-history

A dependency-free Rust price book over the apps' saved receipt observations.
This crate groups and compares purchases; receipt parsing remains in
beanbeaver-core. It has no storage, clock, network or platform bindings.

## What it does

- Groups by `(merchant, item name, optional item code)`. Merchant and name use
  uppercase plus collapsed whitespace; punctuation stays significant. Codes
  retain case and leading zeros. Missing codes never match present codes.
- Applies explicit `ProductLink` aliases, including links across merchants.
  Shared codes alone never merge products. Removing a link and rebuilding undoes
  a merge; conflicting links fail instead of using whichever came last.
- Preserves source IDs, original/display names, recorded quantities, unreadable
  amounts, unknown dates and non-product lines. Repeated lines remain distinct.
- Searches group names, original names, display names, merchants, codes and all
  linked aliases (including aliases without a current observation).
- Produces chronological lists and, for approved observations, per-merchant
  comparable price series with minimum, maximum, quantity-weighted average and
  all observations on the latest purchase date.

## Usage

```rust
use price_history::*;

let item = ItemKey::new("Example Shop", "MILK", Some("000042"))?;
let observation = Observation {
    id: ObservationId { receipt_id: "receipt-1".into(), item_id: "line-1".into() },
    item,
    observed_name: "MILK".into(),
    display_name: Some("Breakfast milk".into()),
    date: Some(PurchaseDate::new(2026, 1, 1)?),
    amount_minor: Some(698),
    currency: Some("CAD".into()),
    recorded_quantity: Some(1),
    kind: LineKind::Product,
    // The scanner's default 1 does not prove that one package was purchased.
    comparison: None,
};
let history = build_history(&[observation.clone()], &[])?;
assert_eq!(search(&history, "breakfast").len(), 1);
assert!(history[0].comparisons.is_empty());

// After checking the receipt and confirming two of the same package:
let mut checked = observation;
checked.comparison = Some(ComparisonApproval {
    units: 2,
    tax: TaxBasis::Excluded,
    discounts: DiscountBasis::BeforeDiscounts,
});
let history = build_history(&[checked], &[])?;
let price = history[0].comparisons[0].average;
assert_eq!((price.numerator(), price.denominator()), (349, 1));
# Ok::<(), HistoryError>(())
```

## Comparison contract

`ComparisonApproval` is an explicit assertion that the amount, count, price
basis and specific package identity have been checked. Never derive it just
from `quantity > 0`, a product code, a classifier category or a green receipt
total. In particular, a generic label such as MEAT remains unapproved. An alias
link confirms product identity but does not itself approve any observation's
amount or quantity.

Only product observations with a known purchase date, known currency, readable
nonnegative amount and approval enter price series. Every excluded entry carries
its `ComparisonIssue`s. A confirmed free purchase is zero; an unreadable amount
is `None`. Discounts, deposits, returns, gift cards and other lines remain in
history without entering product statistics, even if mistakenly approved.

The amount is in the receipt currency's **minor units**, not universally cents.
Currency validation checks only the three-letter shape, not a currency registry.
Different currencies, tax bases, discount bases and merchants always produce
different series. Prices are reduced rational numbers; rounding is a display
choice. The average is sum(amounts) / sum(confirmed units). Checked aggregation
reports overflow rather than wrapping, saturating or using floating point.

This API compares counts of the same package. It does not infer weights, package
sizes, interchangeable products, coupon allocation or tax allocation. Mark
`AfterDiscounts` only when `amount_minor` already represents that checked basis;
the enum does not subtract separate discount lines. Keep separate source discount
lines. If a net comparison requires an adjusted amount distinct from the printed
line, add an explicit adjusted-price/provenance contract before enabling it.

Unknown or placeholder dates map to `None`; never substitute scan dates.
Undated entries sort last and do not enter comparisons. Same-day entries use
stable source IDs for deterministic ordering, without inventing an intraday
purchase order. `latest` therefore includes all comparable points on that date.

## App integration contract

Supply a slim snapshot rather than the full OCR result. Use stable receipt/item
IDs; a persistent array index is insufficient when lines can be inserted or
removed. Exact duplicate observation IDs fail, but equal prices or same-day
purchases are retained. A second photo has a different identity: duplicate
purchase confirmation belongs to the app, which omits confirmed duplicate
observations from the snapshot. Do not silently reuse a spending-exclusion flag
as a history-exclusion policy.

Persist product links in the app and rebuild after edits, deletions or alias
changes. Cache the result per store revision, rather than rebuilding for each
view property access. Invalidate/reconfirm comparison approval when the source
amount, count, item identity, currency or basis changes. Keep the observed tuple
stable when only its display label changes. Replacing the snapshot is the entire
invalidation mechanism; this crate holds no hidden history.

The group title for an unlinked tuple is its normalized name. A linked product
uses `ProductLink.display_name`; individual corrected labels stay on entries and
are searchable. Products without matching observations produce no history,
though all supplied links are validated. Output order is deterministic by `HistoryKey`, then date/source
ID within each group; the UI can choose a frequency or recency presentation.

The crate is not yet exposed through `mobile-ffi` or wired into either app.
Future bindings should use the existing `Spend` type prefix and library, and
add persistence round-trip coverage in both apps. There is no parser change or
phone-runtime claim in this implementation.

Run `cargo test -p price-history` from the workspace root. The synthetic tests
cover identity ambiguity, merge/undo, original-name search, duplicate IDs,
quantity uncertainty, missing data, refunds, exact money, date ties, currency and
basis partitions, edit/delete rebuilding, and overflow. No private fixtures are
copied into this public repository.
