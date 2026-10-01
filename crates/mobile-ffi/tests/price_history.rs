//! The price-history seam, held to the same standard as `conversions.rs`: the
//! arithmetic is `price-history`'s and tested there, so what can go wrong here
//! is a hand-copied field landing in its neighbour. Every figure below is
//! distinct — lowest, highest, typical and average are four different `f64`s —
//! so a swapped pair fails.

use bb_mobile_ffi::{
    spend_price_history, SpendHistoryItem, SpendHistoryKey, SpendHistoryReceipt, SpendItemKey,
    SpendItemKeyKind, SpendPricing, SpendProductLink, SpendTag, SpendUnitsBasis,
};

/// The FFI records carry no `PartialEq`, like the rest of this seam.
fn key_is(key: &SpendItemKey, merchant: &str, kind: SpendItemKeyKind, value: &str) -> bool {
    key.merchant == merchant
        && value == key.value
        && matches!(
            (&key.kind, kind),
            (SpendItemKeyKind::Code, SpendItemKeyKind::Code)
                | (SpendItemKeyKind::Name, SpendItemKeyKind::Name)
        )
}

fn item(description: &str, price: &str) -> SpendHistoryItem {
    SpendHistoryItem {
        description: description.into(),
        item_number: None,
        price: price.into(),
        quantity: 1,
        tags: vec![SpendTag {
            path: "grocery".into(),
            display: "Grocery".into(),
        }],
        is_gift_card: false,
    }
}

fn receipt(
    id: &str,
    merchant: &str,
    iso: &str,
    items: Vec<SpendHistoryItem>,
) -> SpendHistoryReceipt {
    SpendHistoryReceipt {
        id: id.into(),
        merchant: merchant.into(),
        merchant_family: None,
        date_iso: Some(iso.into()),
        date_is_placeholder: false,
        items,
    }
}

/// Five buys at Shop One — unit prices 3.00, 5.00, 5.00, 9.50 and a 10.00 that
/// is two of the 5.00 — plus one coded buy at Shop Two, linked into one product.
fn corpus() -> (Vec<SpendHistoryReceipt>, Vec<SpendProductLink>) {
    let mut coded = item("0042 TEA", "7.25");
    coded.item_number = Some("0042".into());
    let receipts = vec![
        receipt("a", "Shop One", "2026-01-01", vec![item("TEA", "3.00")]),
        receipt("b", "Shop One", "2026-01-02", vec![item("TEA", "5.00")]),
        receipt("c", "Shop One", "2026-01-03", vec![item("TEA", "5.00")]),
        receipt("d", "Shop One", "2026-01-04", vec![item("TEA", "9.50")]),
        receipt(
            "e",
            "Shop One",
            "2026-02-05",
            vec![item("BREAD", "2.00"), item("TEA (2 @ 5.00)", "10.00")],
        ),
        receipt("f", "Shop Two", "2026-01-20", vec![coded]),
    ];
    let links = vec![SpendProductLink {
        id: "tea".into(),
        name: "Breakfast tea".into(),
        members: vec![
            SpendItemKey {
                merchant: "SHOP ONE".into(),
                kind: SpendItemKeyKind::Name,
                value: "TEA".into(),
            },
            SpendItemKey {
                merchant: "SHOP TWO".into(),
                kind: SpendItemKeyKind::Code,
                value: "0042".into(),
            },
        ],
    }];
    (receipts, links)
}

#[test]
fn history_fields_survive_the_round_trip_unswapped() {
    let (receipts, links) = corpus();
    let histories = spend_price_history(receipts, links, String::new());
    assert_eq!(histories.len(), 2); // the tea, and the bread
    let tea = &histories[0];

    assert!(matches!(&tea.key, SpendHistoryKey::Product { id } if id == "tea"));
    assert_eq!(tea.name, "Breakfast tea");
    assert_eq!(tea.receipt_count, 6);
    assert_eq!(tea.purchases.len(), 6);
    assert_eq!(tea.members.len(), 2);
    assert!(key_is(
        &tea.members[0],
        "SHOP ONE",
        SpendItemKeyKind::Name,
        "TEA"
    ));
    assert!(key_is(
        &tea.members[1],
        "SHOP TWO",
        SpendItemKeyKind::Code,
        "0042"
    ));

    let newest = tea.latest.as_ref().unwrap();
    assert_eq!(newest.receipt_id, "e");
    assert_eq!(tea.purchases[0].receipt_id, "e");
    assert_eq!(newest.item_index, 1);
    assert_eq!(newest.merchant, "Shop One");
    assert_eq!(newest.description, "TEA (2 @ 5.00)");
    let date = newest.date.as_ref().unwrap();
    assert_eq!((date.year, date.month, date.day), (2026, 2, 5));
    assert_eq!(newest.amount, Some(10.0));
    assert_eq!(newest.units, 2);
    assert!(matches!(newest.basis, SpendUnitsBasis::Inferred));
    assert_eq!(newest.unit_price, Some(5.0));

    let one = &tea.merchants[0];
    assert_eq!(one.merchant, "Shop One");
    assert_eq!(one.purchase_count, 5);
    assert!(matches!(one.pricing, SpendPricing::Steady));
    assert_eq!(one.latest.as_ref().unwrap().receipt_id, "e");
    assert_eq!(one.lowest, Some(3.0));
    assert_eq!(one.highest, Some(9.5));
    assert_eq!(one.typical, Some(5.0));
    // 32.50 over six units.
    assert_eq!(one.average, Some(5.42));

    let two = &tea.merchants[1];
    assert_eq!(two.merchant, "Shop Two");
    assert_eq!(two.purchase_count, 1);
    assert!(matches!(two.pricing, SpendPricing::Single));
    assert_eq!(two.typical, Some(7.25));
}

#[test]
fn an_unlinked_item_key_comes_back_as_three_strings() {
    let (receipts, _) = corpus();
    let histories = spend_price_history(receipts, vec![], "bread".into());
    assert_eq!(histories.len(), 1);
    match &histories[0].key {
        SpendHistoryKey::Item { key } => {
            assert!(key_is(key, "SHOP ONE", SpendItemKeyKind::Name, "BREAD"))
        }
        SpendHistoryKey::Product { .. } => panic!("unlinked bread came back as a product"),
    }
}

#[test]
fn the_query_filters_and_an_unknown_date_stays_unknown() {
    let (mut receipts, links) = corpus();
    receipts[0].date_is_placeholder = true;
    let tea = spend_price_history(receipts, links, "breakfast".into());
    assert_eq!(tea.len(), 1);
    let placeholder = tea[0]
        .purchases
        .iter()
        .find(|p| p.receipt_id == "a")
        .unwrap();
    assert!(placeholder.date.is_none());
    assert!(spend_price_history(vec![], vec![], "anything".into()).is_empty());
}
