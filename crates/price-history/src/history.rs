use std::collections::{BTreeMap, BTreeSet};

use crate::types::{normalize, *};

/// Build from the caller's complete current snapshot, including its explicit
/// product links. No state is retained between calls: edits, removals and undo
/// cannot leave a stale index behind. Cache this result per app-store revision.
///
/// Exact duplicate observation IDs are an error, even if their values agree.
/// A recaptured receipt with a new ID needs app-side duplicate confirmation;
/// this function never deduplicates merely equal merchant/date/amount values.
pub fn build_history(
    observations: &[Observation],
    products: &[ProductLink],
) -> Result<Vec<ItemHistory>, HistoryError> {
    let mut product_ids = BTreeSet::new();
    let mut aliases: BTreeMap<&ItemKey, &ProductLink> = BTreeMap::new();
    for product in products {
        if product.id.trim().is_empty()
            || product.display_name.trim().is_empty()
            || product.aliases.is_empty()
        {
            return Err(HistoryError::InvalidProductLink(product.id.clone()));
        }
        if !product_ids.insert(&product.id) {
            return Err(HistoryError::DuplicateProductId(product.id.clone()));
        }
        for alias in &product.aliases {
            if let Some(prior) = aliases.insert(alias, product) {
                if prior.id != product.id {
                    return Err(HistoryError::ConflictingAlias(alias.clone()));
                }
            }
        }
    }

    let mut seen = BTreeSet::new();
    let mut groups = BTreeMap::new();
    for observation in observations {
        validate(observation)?;
        if !seen.insert(&observation.id) {
            return Err(HistoryError::DuplicateObservation(observation.id.clone()));
        }
        let product = aliases.get(&observation.item);
        let key = match product {
            Some(p) => HistoryKey::Product(p.id.clone()),
            None => HistoryKey::Item(observation.item.clone()),
        };
        let group = groups.entry(key.clone()).or_insert_with(|| {
            let aliases = match product {
                Some(p) => p
                    .aliases
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                None => vec![observation.item.clone()],
            };
            ItemHistory {
                key,
                display_name: product.map_or_else(
                    || observation.item.name().to_owned(),
                    |p| p.display_name.clone(),
                ),
                aliases,
                entries: Vec::new(),
                receipt_count: 0,
                comparisons: Vec::new(),
            }
        });
        group.entries.push(HistoryEntry {
            observation: observation.clone(),
            comparison_issues: comparison_issues(observation),
        });
    }

    groups
        .into_values()
        .map(|mut group| {
            group.entries.sort_by(|a, b| {
                let a = &a.observation;
                let b = &b.observation;
                (a.date.is_none(), a.date, &a.id).cmp(&(b.date.is_none(), b.date, &b.id))
            });
            group.receipt_count = group
                .entries
                .iter()
                .map(|entry| &entry.observation.id.receipt_id)
                .collect::<BTreeSet<_>>()
                .len();
            let mut series: BTreeMap<SeriesKey, Vec<PricePoint>> = BTreeMap::new();
            for entry in &group.entries {
                if !entry.comparison_issues.is_empty() {
                    continue;
                }
                let observation = &entry.observation;
                // The eligibility check and validation above establish every field.
                let approval = observation.comparison.as_ref().expect("approved entry");
                let amount = observation.amount_minor.expect("readable amount") as u64;
                let key = SeriesKey {
                    merchant: observation.item.merchant().to_owned(),
                    currency: normalize(observation.currency.as_ref().expect("known currency")),
                    tax: approval.tax,
                    discounts: approval.discounts,
                };
                series.entry(key).or_default().push(PricePoint {
                    source: observation.id.clone(),
                    date: observation.date.expect("known date"),
                    amount_minor: amount,
                    units: approval.units,
                    unit_price: UnitPrice::new(amount, u64::from(approval.units)),
                });
            }
            group.comparisons = series
                .into_iter()
                .map(|(key, points)| {
                    let mut amount = 0_u64;
                    let mut units = 0_u64;
                    for point in &points {
                        amount = amount
                            .checked_add(point.amount_minor)
                            .ok_or(HistoryError::ArithmeticOverflow)?;
                        units = units
                            .checked_add(u64::from(point.units))
                            .ok_or(HistoryError::ArithmeticOverflow)?;
                    }
                    // A series is created only when a point is added; entries were sorted.
                    let latest_date = points.last().expect("nonempty series").date;
                    let latest = points
                        .iter()
                        .filter(|p| p.date == latest_date)
                        .cloned()
                        .collect();
                    let minimum = points
                        .iter()
                        .map(|p| p.unit_price)
                        .min()
                        .expect("nonempty series");
                    let maximum = points
                        .iter()
                        .map(|p| p.unit_price)
                        .max()
                        .expect("nonempty series");
                    Ok(PriceSeries {
                        key,
                        points,
                        minimum,
                        maximum,
                        average: UnitPrice::new(amount, units),
                        latest,
                    })
                })
                .collect::<Result<_, HistoryError>>()?;
            Ok(group)
        })
        .collect()
}

fn validate(observation: &Observation) -> Result<(), HistoryError> {
    let id = &observation.id;
    if id.receipt_id.trim().is_empty() || id.item_id.trim().is_empty() {
        return Err(HistoryError::EmptyObservationId);
    }
    if let Some(currency) = &observation.currency {
        let currency = currency.trim();
        if currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(HistoryError::InvalidCurrency(id.clone()));
        }
    }
    if observation
        .comparison
        .as_ref()
        .is_some_and(|a| a.units == 0)
    {
        return Err(HistoryError::ZeroApprovedUnits(id.clone()));
    }
    Ok(())
}

fn comparison_issues(observation: &Observation) -> Vec<ComparisonIssue> {
    let mut issues = Vec::new();
    if observation.kind != LineKind::Product {
        issues.push(ComparisonIssue::NotProduct);
    }
    if observation.date.is_none() {
        issues.push(ComparisonIssue::UnknownDate);
    }
    if observation.currency.is_none() {
        issues.push(ComparisonIssue::UnknownCurrency);
    }
    match observation.amount_minor {
        None => issues.push(ComparisonIssue::UnreadableAmount),
        Some(value) if value < 0 => issues.push(ComparisonIssue::NegativeAmount),
        _ => {}
    }
    if observation.comparison.is_none() {
        issues.push(ComparisonIssue::Unapproved);
    }
    issues
}

/// Case/whitespace-normalized substring search over group names, aliases,
/// merchants, codes, original line names and user display names. An empty query
/// returns everything in the supplied order; this is not fuzzy product matching.
pub fn search<'a>(histories: &'a [ItemHistory], query: &str) -> Vec<&'a ItemHistory> {
    let query = normalize(query);
    let matches = |text: &str| normalize(text).contains(&query);
    histories
        .iter()
        .filter(|history| {
            matches(&history.display_name)
                || history.aliases.iter().any(|key| {
                    matches(key.merchant())
                        || matches(key.name())
                        || key.code().is_some_and(matches)
                })
                || history.entries.iter().any(|entry| {
                    matches(&entry.observation.observed_name)
                        || entry
                            .observation
                            .display_name
                            .as_deref()
                            .is_some_and(matches)
                })
        })
        .collect()
}
