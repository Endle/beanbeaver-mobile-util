//! The specification for [`price_history`]: which lines are one item, how many
//! units each amount paid for, and when a price is steady enough to compare.
//!
//! Every name, code and price here is invented. The rules were measured on the
//! private receipt corpus (see `Pricing`'s docs); its receipts stay private.

use price_history::{
    clean_name, matches, price_history, purchase_date, HistoryItem, HistoryKey, HistoryReceipt,
    Identity, ItemHistory, ItemKey, ItemTag, Pricing, ProductLink, SpendDate, UnitsBasis,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn item(description: &str, price: &str) -> HistoryItem {
    HistoryItem {
        description: description.into(),
        item_number: None,
        price: price.into(),
        quantity: 1,
        tags: vec![ItemTag::new("grocery", "Grocery")],
        is_gift_card: false,
    }
}

fn coded(code: &str, description: &str, price: &str) -> HistoryItem {
    HistoryItem {
        item_number: Some(code.into()),
        ..item(description, price)
    }
}

fn receipt(id: &str, merchant: &str, iso: &str, items: Vec<HistoryItem>) -> HistoryReceipt {
    HistoryReceipt {
        id: id.into(),
        merchant: merchant.into(),
        merchant_family: None,
        date_iso: Some(iso.into()),
        date_is_placeholder: false,
        items,
    }
}

/// One receipt per price, a day apart, each buying one line of `description`.
fn buys(merchant: &str, description: &str, prices: &[&str]) -> Vec<HistoryReceipt> {
    prices
        .iter()
        .enumerate()
        .map(|(i, price)| {
            receipt(
                &format!("{merchant}-{i}"),
                merchant,
                &format!("2026-01-{:02}", i + 1),
                vec![item(description, price)],
            )
        })
        .collect()
}

fn name_key(merchant: &str, name: &str) -> ItemKey {
    ItemKey {
        merchant: merchant.into(),
        identity: Identity::Name(name.into()),
    }
}

fn code_key(merchant: &str, code: &str) -> ItemKey {
    ItemKey {
        merchant: merchant.into(),
        identity: Identity::Code(code.into()),
    }
}

fn only(histories: &[ItemHistory]) -> &ItemHistory {
    assert_eq!(histories.len(), 1, "{histories:#?}");
    &histories[0]
}

fn day(y: i32, m: u32, d: u32) -> SpendDate {
    SpendDate::new(y, m, d)
}

// ---------------------------------------------------------------------------
// What is one item
// ---------------------------------------------------------------------------

#[test]
fn a_printed_code_is_the_identity_and_the_name_only_a_label() {
    let receipts = vec![
        receipt(
            "a",
            "Warehouse",
            "2026-01-01",
            vec![coded("4521", "4521 2% MILK-FILT", "5.29")],
        ),
        receipt(
            "b",
            "Warehouse",
            "2026-01-08",
            vec![coded("4521", "4521 2% MILK/FILT", "5.29")],
        ),
        receipt(
            "c",
            "Warehouse",
            "2026-01-15",
            vec![coded("4521", "2% MILK FILT", "5.29")],
        ),
    ];
    let history = price_history(&receipts, &[]);
    let milk = only(&history);
    assert_eq!(milk.key, HistoryKey::Item(code_key("WAREHOUSE", "4521")));
    assert_eq!(milk.purchases.len(), 3);
    // The code is stripped from the label; every spelling appears once, so the
    // latest wins the title.
    assert_eq!(milk.name, "2% MILK FILT");
}

#[test]
fn a_line_missing_its_code_joins_the_one_code_printed_with_that_name() {
    let receipts = vec![
        receipt(
            "a",
            "Warehouse",
            "2026-01-01",
            vec![coded("812", "812 LG EGGS", "6.49")],
        ),
        receipt(
            "b",
            "Warehouse",
            "2026-01-08",
            vec![item("LG EGGS", "6.49")],
        ),
    ];
    let eggs = only(&price_history(&receipts, &[])).clone();
    assert_eq!(eggs.key, HistoryKey::Item(code_key("WAREHOUSE", "812")));
    assert_eq!(eggs.members, vec![code_key("WAREHOUSE", "812")]);
}

#[test]
fn a_name_printed_with_two_codes_is_ambiguous_and_stays_by_name() {
    let receipts = vec![
        receipt(
            "a",
            "Warehouse",
            "2026-01-01",
            vec![coded("1", "1 MILK", "5.00")],
        ),
        receipt(
            "b",
            "Warehouse",
            "2026-01-02",
            vec![coded("2", "2 MILK", "9.00")],
        ),
        receipt("c", "Warehouse", "2026-01-03", vec![item("MILK", "5.00")]),
    ];
    let history = price_history(&receipts, &[]);
    assert_eq!(history.len(), 3);
    assert!(history
        .iter()
        .any(|h| h.key == HistoryKey::Item(name_key("WAREHOUSE", "MILK"))));
}

#[test]
fn codes_never_join_across_merchants() {
    let receipts = vec![
        receipt(
            "a",
            "Shop One",
            "2026-01-01",
            vec![coded("0627", "0627 YOGURT", "3.19")],
        ),
        receipt(
            "b",
            "Shop Two",
            "2026-01-02",
            vec![coded("0627", "0627 YOGURT", "2.99")],
        ),
    ];
    assert_eq!(price_history(&receipts, &[]).len(), 2);
}

#[test]
fn deal_text_markers_case_and_spacing_do_not_split_an_item() {
    let receipts = vec![
        receipt(
            "a",
            "Corner Mart",
            "2026-01-01",
            vec![item("Rice Crackers ((300g)@3.49(1/$1.89))", "1.89")],
        ),
        receipt(
            "b",
            "corner  mart",
            "2026-01-02",
            vec![item("*rice crackers ((## 300g)@3.49(2/$4.00))", "2.00")],
        ),
        receipt(
            "c",
            "CORNER MART",
            "2026-01-03",
            vec![item("RICE   CRACKERS", "3.49")],
        ),
    ];
    let eggs = only(&price_history(&receipts, &[])).clone();
    assert_eq!(
        eggs.key,
        HistoryKey::Item(name_key("CORNER MART", "RICE CRACKERS"))
    );
    assert_eq!(eggs.purchases.len(), 3);
    // Purchases keep what was printed; only the key is cleaned.
    assert_eq!(
        eggs.purchases[1].description,
        "*rice crackers ((## 300g)@3.49(2/$4.00))"
    );
}

#[test]
fn clean_name_takes_off_deal_text_and_keeps_printed_brackets() {
    assert_eq!(
        clean_name("AB - Rice Crackers ((300g)@3.49(1/$1.89))", None),
        "AB - Rice Crackers"
    );
    assert_eq!(clean_name("HOUSE RED (3 @ 12.50)", None), "HOUSE RED");
    assert_eq!(
        clean_name("Soup Base 100gx4) @5.49 (1/$3.99)", None),
        "Soup Base 100gx4)"
    );
    assert_eq!(clean_name("812 LG EGGS", Some("812")), "LG EGGS");
    // A name that merely starts with the code's digits is not a code prefix.
    assert_eq!(clean_name("8121 THING", Some("812")), "8121 THING");
    assert_eq!(clean_name("*Bok Choy (Small)", None), "Bok Choy (Small)");
    // All annotation: returned whole rather than empty.
    assert_eq!(clean_name("@ 2.00", None), "@ 2.00");
}

#[test]
fn a_recognised_family_keys_the_merchant_however_the_header_read() {
    let mut a = receipt("a", "WHOLESALE", "2026-01-01", vec![item("BREAD", "4.00")]);
    a.merchant_family = Some("Warehouse".into());
    let b = receipt("b", "Warehouse", "2026-01-02", vec![item("BREAD", "4.00")]);
    let mut c = receipt("c", "Warehouse", "2026-01-03", vec![item("BREAD", "4.00")]);
    c.merchant_family = Some("  ".into()); // blank is no family
    let bread = only(&price_history(&[a, b, c], &[])).clone();
    assert_eq!(bread.purchases.len(), 3);
    assert!(bread.purchases.iter().all(|p| p.merchant == "Warehouse"));
}

#[test]
fn only_purchases_are_history() {
    let mut deposit = item("DEPOSIT 1", "0.10");
    deposit.tags = vec![ItemTag::new("deposit", "Deposit")];
    let mut discount = item("TPD/812", "3.00"); // even unsigned
    discount.tags = vec![ItemTag::new("discount", "Discount")];
    let mut gift = item("GIFT CARD", "25.00");
    gift.is_gift_card = true;
    let receipts = vec![receipt(
        "a",
        "Shop",
        "2026-01-01",
        vec![
            deposit,
            discount,
            gift,
            item("RETURNED KETTLE", "-30.00"),
            item("BANNER", "0.00"),
            item("SMUDGED", "N/A"),
            item("TEA", "4.00"),
        ],
    )];
    let history = price_history(&receipts, &[]);
    let mut names: Vec<&str> = history.iter().map(|h| h.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["SMUDGED", "TEA"]);
    let smudged = history.iter().find(|h| h.name == "SMUDGED").unwrap();
    assert_eq!(smudged.purchases[0].amount, None);
    assert_eq!(smudged.purchases[0].item_index, 5);
    assert_eq!(smudged.latest, None);
    assert_eq!(smudged.merchants[0].pricing, Pricing::Single);
    assert_eq!(smudged.merchants[0].average, None);
}

// ---------------------------------------------------------------------------
// How many units an amount paid for
// ---------------------------------------------------------------------------

#[test]
fn a_multiple_of_another_price_is_a_multi_buy_not_a_price_rise() {
    let receipts = buys("Shop", "EGGS", &["6.49", "6.49", "12.98", "6.49"]);
    let eggs = only(&price_history(&receipts, &[])).clone();
    let double = eggs
        .purchases
        .iter()
        .find(|p| p.amount == Some(1298))
        .unwrap();
    assert_eq!((double.units, double.basis), (2, UnitsBasis::Inferred));
    assert_eq!(double.unit_price, Some(649));
    let prices = &eggs.merchants[0];
    assert_eq!(prices.pricing, Pricing::Steady);
    assert_eq!(
        (prices.lowest, prices.highest, prices.typical),
        (Some(649), Some(649), Some(649))
    );
    assert_eq!(prices.average, Some(649));
}

#[test]
fn the_smallest_dividing_price_sets_the_count() {
    // 45.96 beside 11.49 and 22.98 is four of the first, not two of the second.
    let receipts = buys(
        "Shop",
        "PRAWNS",
        &["11.49", "22.98", "34.47", "45.96", "11.49"],
    );
    let prawns = only(&price_history(&receipts, &[])).clone();
    let units: Vec<u32> = prawns.purchases.iter().rev().map(|p| p.units).collect();
    assert_eq!(units, [1, 2, 3, 4, 1]);
    assert!(prawns.purchases.iter().all(|p| p.unit_price == Some(1149)));
}

#[test]
fn a_divisible_amount_among_prices_that_never_repeat_is_left_alone() {
    // By-weight amounts: 8.00 happens to be 2 × 4.00, but nothing else repeats,
    // so it is not evidence of a multi-buy.
    let receipts = buys(
        "Butcher",
        "MEAT",
        &["4.00", "13.27", "8.00", "6.51", "17.03"],
    );
    let meat = only(&price_history(&receipts, &[])).clone();
    assert!(meat
        .purchases
        .iter()
        .all(|p| p.basis == UnitsBasis::Assumed && p.units == 1));
    let prices = &meat.merchants[0];
    assert_eq!(prices.pricing, Pricing::Varies);
    assert_eq!((prices.lowest, prices.highest), (Some(400), Some(1703)));
}

#[test]
fn a_recorded_quantity_counts_and_seeds_inference() {
    let mut six = item("BUNS", "9.54");
    six.quantity = 6;
    let receipts = vec![
        receipt("a", "Bakery", "2026-01-01", vec![six]),
        receipt("b", "Bakery", "2026-01-02", vec![item("BUNS", "9.54")]),
    ];
    let buns = only(&price_history(&receipts, &[])).clone();
    let recorded = buns.purchases.iter().find(|p| p.receipt_id == "a").unwrap();
    assert_eq!((recorded.units, recorded.basis), (6, UnitsBasis::Recorded));
    let inferred = buns.purchases.iter().find(|p| p.receipt_id == "b").unwrap();
    assert_eq!((inferred.units, inferred.basis), (6, UnitsBasis::Inferred));
    assert_eq!(buns.merchants[0].pricing, Pricing::Steady);
    assert_eq!(buns.merchants[0].typical, Some(159));
}

#[test]
fn a_negative_or_zero_quantity_is_one() {
    let mut odd = item("TEA", "4.00");
    odd.quantity = -2;
    let mut zero = item("TEA", "4.00");
    zero.quantity = 0;
    let receipts = vec![receipt("a", "Shop", "2026-01-01", vec![odd, zero])];
    let tea = only(&price_history(&receipts, &[])).clone();
    assert!(tea
        .purchases
        .iter()
        .all(|p| p.units == 1 && p.basis == UnitsBasis::Assumed));
}

#[test]
fn units_are_inferred_within_a_merchant_only() {
    let mut receipts = buys("Shop One", "COLA", &["2.00", "2.00"]);
    receipts.extend(buys("Shop Two", "COLA", &["4.00"]));
    let link = ProductLink {
        id: "cola".into(),
        name: "Cola".into(),
        members: vec![name_key("Shop One", "COLA"), name_key("Shop Two", "COLA")],
    };
    let cola = only(&price_history(&receipts, &[link])).clone();
    let two = cola
        .purchases
        .iter()
        .find(|p| p.merchant == "Shop Two")
        .unwrap();
    assert_eq!((two.units, two.basis), (1, UnitsBasis::Assumed));
}

// ---------------------------------------------------------------------------
// When a price is a price
// ---------------------------------------------------------------------------

#[test]
fn pricing_is_steady_when_at_least_half_the_prices_repeat() {
    let pricing = |prices: &[&str]| {
        only(&price_history(&buys("Shop", "X", prices), &[])).merchants[0].pricing
    };
    assert_eq!(pricing(&["5.00"]), Pricing::Single);
    assert_eq!(pricing(&["5.00", "5.00"]), Pricing::Steady);
    // A rise and a weighed item look the same until a price comes round again.
    assert_eq!(pricing(&["4.85", "5.29"]), Pricing::Varies);
    assert_eq!(pricing(&["4.85", "4.85", "5.29"]), Pricing::Steady);
    assert_eq!(pricing(&["4.85", "4.85", "5.29", "7.10"]), Pricing::Steady);
    assert_eq!(
        pricing(&["4.85", "4.85", "5.29", "7.10", "7.45"]),
        Pricing::Varies
    );
}

#[test]
fn typical_is_the_most_common_price_and_the_latest_breaks_a_tie() {
    let receipts = buys("Shop", "MILK", &["4.85", "4.85", "5.29", "5.29"]);
    let milk = only(&price_history(&receipts, &[])).clone();
    assert_eq!(milk.merchants[0].typical, Some(529));
    assert_eq!(milk.merchants[0].average, Some(507));
}

#[test]
fn the_average_weighs_each_unit_and_rounds_half_up() {
    let mut three = item("PEARS", "5.00");
    three.quantity = 3;
    let receipts = vec![
        receipt("a", "Shop", "2026-01-01", vec![three]),
        receipt("b", "Shop", "2026-01-02", vec![item("PEARS", "2.00")]),
    ];
    let pears = only(&price_history(&receipts, &[])).clone();
    // 500 / 3 = 166.67 → 167.
    assert_eq!(pears.purchases[1].unit_price, Some(167));
    // 700 / 4 = 175.
    assert_eq!(pears.merchants[0].average, Some(175));
}

// ---------------------------------------------------------------------------
// Dates and order
// ---------------------------------------------------------------------------

#[test]
fn a_purchase_date_is_never_invented() {
    let mut placeholder = receipt("p", "Shop", "2026-01-01", vec![]);
    placeholder.date_is_placeholder = true;
    assert_eq!(purchase_date(&placeholder), None);
    assert_eq!(
        purchase_date(&receipt("x", "Shop", "2026-13-01", vec![])),
        None
    );
    let mut missing = receipt("m", "Shop", "", vec![]);
    missing.date_iso = None;
    assert_eq!(purchase_date(&missing), None);
    assert_eq!(
        purchase_date(&receipt("ok", "Shop", "2026-02-28", vec![])),
        Some(day(2026, 2, 28))
    );
}

#[test]
fn undated_purchases_sort_last_count_in_figures_and_are_never_latest() {
    let mut undated = receipt("u", "Shop", "", vec![item("TEA", "1.00")]);
    undated.date_iso = None;
    let receipts = vec![
        undated,
        receipt("old", "Shop", "2026-01-01", vec![item("TEA", "4.00")]),
        receipt("new", "Shop", "2026-02-01", vec![item("TEA", "4.00")]),
    ];
    let tea = only(&price_history(&receipts, &[])).clone();
    let ids: Vec<&str> = tea
        .purchases
        .iter()
        .map(|p| p.receipt_id.as_str())
        .collect();
    assert_eq!(ids, ["new", "old", "u"]);
    assert_eq!(tea.latest.as_ref().unwrap().receipt_id, "new");
    assert_eq!(tea.merchants[0].lowest, Some(100));
}

#[test]
fn latest_skips_an_unreadable_price() {
    let receipts = vec![
        receipt("old", "Shop", "2026-01-01", vec![item("TEA", "4.00")]),
        receipt("new", "Shop", "2026-02-01", vec![item("TEA", "N/A")]),
    ];
    let tea = only(&price_history(&receipts, &[])).clone();
    assert_eq!(tea.purchases[0].receipt_id, "new");
    assert_eq!(tea.latest.unwrap().receipt_id, "old");
}

#[test]
fn repeated_lines_on_one_receipt_are_separate_purchases() {
    let receipts = vec![receipt(
        "a",
        "Shop",
        "2026-01-01",
        vec![
            item("MILK", "5.29"),
            item("BREAD", "3.00"),
            item("MILK", "5.29"),
        ],
    )];
    let history = price_history(&receipts, &[]);
    let milk = history.iter().find(|h| h.name == "MILK").unwrap();
    let indexes: Vec<u32> = milk.purchases.iter().map(|p| p.item_index).collect();
    assert_eq!(indexes, [0, 2]);
    assert_eq!(milk.receipt_count, 1);
}

#[test]
fn histories_are_most_recent_first_and_independent_of_input_order() {
    let receipts = vec![
        receipt("a", "Shop", "2026-01-01", vec![item("OLD", "1.00")]),
        receipt("b", "Shop", "2026-03-01", vec![item("NEW", "1.00")]),
        receipt(
            "c",
            "Shop",
            "2026-02-01",
            vec![item("MID", "1.00"), item("MID", "1.00")],
        ),
    ];
    let forward = price_history(&receipts, &[]);
    let names: Vec<&str> = forward.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["NEW", "MID", "OLD"]);
    let mut reversed = receipts.clone();
    reversed.reverse();
    assert_eq!(price_history(&reversed, &[]), forward);
}

// ---------------------------------------------------------------------------
// The user's links
// ---------------------------------------------------------------------------

#[test]
fn a_link_joins_merchants_without_mixing_their_prices() {
    let mut receipts = buys("Shop One", "YOGURT 750G", &["3.19", "3.19"]);
    receipts.extend(buys("Shop Two", "NAT YOGURT", &["2.99"]));
    let link = ProductLink {
        id: "yogurt".into(),
        name: "Plain yogurt".into(),
        // As a reader typed it back: unfolded keys still match.
        members: vec![
            name_key("shop one", "yogurt  750g"),
            name_key("SHOP TWO", "NAT YOGURT"),
        ],
    };
    let yogurt = only(&price_history(&receipts, &[link])).clone();
    assert_eq!(yogurt.key, HistoryKey::Product("yogurt".into()));
    assert_eq!(yogurt.name, "Plain yogurt");
    assert_eq!(yogurt.members.len(), 2);
    assert_eq!(yogurt.merchants.len(), 2);
    assert_eq!(yogurt.merchants[0].merchant, "Shop One");
    assert_eq!(yogurt.merchants[1].typical, Some(299));
    // Removing the link is the undo.
    assert_eq!(price_history(&receipts, &[]).len(), 2);
}

#[test]
fn a_single_member_link_renames_and_a_blank_name_falls_back() {
    let receipts = buys("Shop", "ORG MILK 4L", &["6.00"]);
    let mut link = ProductLink {
        id: "milk".into(),
        name: "Organic milk".into(),
        members: vec![name_key("SHOP", "ORG MILK 4L")],
    };
    assert_eq!(
        only(&price_history(&receipts, &[link.clone()])).name,
        "Organic milk"
    );
    link.name = "   ".into();
    assert_eq!(only(&price_history(&receipts, &[link])).name, "ORG MILK 4L");
}

#[test]
fn a_key_claimed_twice_goes_to_the_smaller_id_whatever_the_order() {
    let receipts = buys("Shop", "TEA", &["4.00"]);
    let link = |id: &str| ProductLink {
        id: id.into(),
        name: id.into(),
        members: vec![name_key("SHOP", "TEA")],
    };
    for links in [vec![link("b"), link("a")], vec![link("a"), link("b")]] {
        assert_eq!(
            only(&price_history(&receipts, &links)).key,
            HistoryKey::Product("a".into())
        );
    }
}

#[test]
fn a_link_with_no_purchases_produces_nothing() {
    let link = ProductLink {
        id: "gone".into(),
        name: "Gone".into(),
        members: vec![name_key("SHOP", "NOTHING")],
    };
    assert!(price_history(&buys("Shop", "TEA", &["4.00"]), &[link])
        .iter()
        .all(|h| h.key != HistoryKey::Product("gone".into())));
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[test]
fn search_reads_names_codes_merchants_and_printed_text() {
    let receipts = vec![receipt(
        "a",
        "Warehouse",
        "2026-01-01",
        vec![coded("0812", "0812 LG EGGS (2/$9.00 @)", "6.49")],
    )];
    let eggs = only(&price_history(&receipts, &[])).clone();
    for query in ["", "lg  eggs", "0812", "warehouse", "2/$9.00"] {
        assert!(matches(&eggs, query), "{query}");
    }
    assert!(!matches(&eggs, "milk"));
}
