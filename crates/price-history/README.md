# price-history

What an item cost each time it was bought, from the receipts both apps already
store. The rules — what counts as one item, how many units a line paid for, and
when a price is steady enough to compare — are in the crate docs
(`cargo doc -p price-history --open`), next to the code that applies them.

`spend_price_history` in `crates/mobile-ffi` is the seam. Neither app calls it
yet.

## What a screen may claim

| Field | Show | Don't |
|---|---|---|
| Every purchase | amount, date, merchant, a link to its receipt | hide one because its price is unreadable or it has no date |
| `basis == Inferred` | "2 × $6.49", marked as a reading | present it as printed |
| `pricing == Steady` | the typical price, and the latest against it ("↑ $0.40 on usual") | — |
| `pricing == Varies` | what was paid, lowest and highest | a trend, an average as "the price", "↑ $x" |
| `pricing == Single` | the one price | any comparison |
| Two merchants | each merchant's figures side by side | one figure across both: they never mix |

`date == None` means the receipt's date was missing or a placeholder. It is
never the scan date, and an undated purchase is never `latest`.

## Integrating an app

- **Project, don't pass the record.** Map each stored receipt into
  `SpendHistoryReceipt` — about ten lines, like `SpendInput`. `merchant_family`
  is `merchantMatch.canonical`; `item_index` in the output is the index into
  the receipt's stored items.
- **Pass every receipt.** Spending's `isExcluded` means "not my spending", not
  "not a real price", so excluded receipts belong here too. A second photo of
  one receipt shows up as two purchases on one date; nothing here guesses that
  they are the same purchase.
- **Cache per store revision.** One call builds every history. Do not make it a
  computed property re-read per view access — the cost measured on iOS's
  spend screens (`SpendStore`) was the call count, not the work per call.
- **Persist links, nothing else.** A `SpendProductLink` is three strings per
  member plus an id and a name. Merge = a link with more members; split = fewer;
  rename = a one-member link with a name. A member no receipt produces any more
  is harmless.
- **Stored scans keep their descriptions.** A core release that improves a
  description changes new scans' keys, not old ones'. A link is how the user
  joins the two.

## Not yet

- No package sizes or weights, so no price per kilogram or per litre, and
  linking a 2 L carton to a 4 L one makes a history whose prices don't compare.
- Discounts are excluded, not netted against the item they follow.
- One currency: amounts are compared as printed.
- Cross-merchant suggestions ("this code is also sold at …") — links are
  manual.
