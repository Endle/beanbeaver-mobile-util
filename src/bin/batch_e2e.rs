//! Host-side twin of the on-device BatchRunner (iOS / Android).
//!
//! Scans every `*.jpg` in `--in-dir` through `bb-receipt-ffi` (same core the
//! mobile apps call) and writes `batch_out.json` for `compare-e2e.py`.
//!
//!   cargo run --release --bin batch_e2e -- \
//!     --models models --in-dir /path/to/batch_in --out batch_out.json

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bb_receipt_ffi::{DateYmd, OcrSession, Phase, ReceiptWarningKind, ScanTimings};

/// Ledger defaults the app passes on every scan (see `ReceiptScanner.kt`);
/// mirrored here so the host results are graded against the same parse the
/// device would produce.
const CURRENCY: &str = "CAD";
const TAX_ACCOUNT: &str = "Expenses:Tax:HST";
const CREDIT_CARD_ACCOUNT: &str = "Liabilities:CreditCard";

fn main() {
    let mut models = PathBuf::from("models");
    let mut in_dir = PathBuf::from("batch_in");
    let mut out = PathBuf::from("batch_out.json");
    let mut use_cls = true;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--models" => models = PathBuf::from(args.next().expect("--models value")),
            "--in-dir" => in_dir = PathBuf::from(args.next().expect("--in-dir value")),
            "--out" => out = PathBuf::from(args.next().expect("--out value")),
            "--no-orientation-cls" => use_cls = false,
            other => panic!("unknown arg: {other}"),
        }
    }

    let session = OcrSession::new(models.to_string_lossy().into_owned(), use_cls)
        .unwrap_or_else(|e| panic!("load models from {}: {e}", models.display()));

    let mut jpgs: Vec<PathBuf> = fs::read_dir(&in_dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", in_dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("jpg"))
                .unwrap_or(false)
        })
        .collect();
    jpgs.sort();

    let today = {
        // Match mobile: local calendar date for date inference / placeholder.
        let now = time_ymd_local();
        DateYmd {
            year: now.0,
            month: now.1,
            day: now.2,
        }
    };

    let mut results = Vec::new();
    for path in &jpgs {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                results.push(failure_json(&name, &format!("load failed: {e}")));
                continue;
            }
        };
        let started = Instant::now();
        match session.scan(
            bytes,
            DateYmd {
                year: today.year,
                month: today.month,
                day: today.day,
            },
            CREDIT_CARD_ACCOUNT.into(),
            CURRENCY.into(),
            TAX_ACCOUNT.into(),
        ) {
            Ok(r) => {
                let wall_ms = started.elapsed().as_secs_f64() * 1e3;
                results.push(success_json(&name, &r, wall_ms));
                eprintln!(
                    "ok {name} wall_ms={wall_ms:.0} merchant={} total={}",
                    r.merchant, r.total
                );
            }
            Err(e) => {
                results.push(failure_json(&name, &e.to_string()));
                eprintln!("fail {name}: {e}");
            }
        }
    }

    let body = format!(
        "{{\n  \"count\": {},\n  \"results\": [\n{}\n  ]\n}}\n",
        results.len(),
        results.join(",\n")
    );
    if let Some(parent) = out.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&out, body).unwrap_or_else(|e| panic!("write {}: {e}", out.display()));
    eprintln!("wrote {} result(s) → {}", results.len(), out.display());
}

fn time_ymd_local() -> (i32, u32, u32) {
    // Avoid extra deps: parse `date +%Y-%m-%d` when available; else 1970-01-01.
    if let Ok(out) = std::process::Command::new("date")
        .args(["+%Y-%m-%d"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut parts = s.trim().split('-');
            if let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) {
                if let (Ok(y), Ok(m), Ok(d)) = (y.parse(), m.parse(), d.parse()) {
                    return (y, m, d);
                }
            }
        }
    }
    (1970, 1, 1)
}

fn success_json(name: &str, r: &bb_receipt_ffi::ReceiptResult, wall_ms: f64) -> String {
    let items: Vec<String> = r
        .items
        .iter()
        .map(|i| {
            format!(
                "        {{\"description\":{},\"price\":{},\"account\":{}}}",
                json_str(&i.description),
                json_str(&i.price),
                match &i.account {
                    Some(c) => json_str(c),
                    None => "null".into(),
                }
            )
        })
        .collect();
    // Schema unchanged for compare-e2e.py: the messages, and only the kinds
    // this list carried before v0.8.0 gave warnings a kind. The app-side twin
    // is `WarningSeverity.worthShowing`; spelled out here because this bin is a
    // host harness with no Kotlin in reach.
    let warnings: Vec<String> = r
        .warnings
        .iter()
        .filter(|w| worth_showing(w.kind))
        .map(|w| json_str(&w.message))
        .collect();
    format!(
        r#"    {{
      "name": {name},
      "merchant": {merchant},
      "date": {date},
      "dateIsPlaceholder": {placeholder},
      "total": {total},
      "subtotal": {subtotal},
      "tax": {tax},
      "items": [
{items}
      ],
      "warnings": [{warnings}],
      "wallMs": {wall_ms},
      "timings": {{
        "decodeMs": {decode},
        "prepMs": {prep},
        "detectMs": {detect},
        "classifyMs": {classify},
        "recognizeMs": {recognize},
        "parseMs": {parse},
        "totalMs": {total_ms}
      }},
      "error": null
    }}"#,
        name = json_str(name),
        merchant = json_str(&r.merchant),
        date = r
            .date
            .as_ref()
            .map(|d| json_str(d))
            .unwrap_or_else(|| "null".into()),
        placeholder = r.date_is_placeholder,
        total = json_str(&r.total),
        subtotal = r
            .subtotal
            .as_ref()
            .map(|d| json_str(d))
            .unwrap_or_else(|| "null".into()),
        tax = r
            .tax
            .as_ref()
            .map(|d| json_str(d))
            .unwrap_or_else(|| "null".into()),
        items = items.join(",\n"),
        warnings = warnings.join(","),
        wall_ms = wall_ms,
        decode = phase_ms(&r.timings, Phase::Decode),
        prep = phase_ms(&r.timings, Phase::Prep),
        detect = phase_ms(&r.timings, Phase::Detect),
        classify = phase_ms(&r.timings, Phase::Classify),
        recognize = phase_ms(&r.timings, Phase::Recognize),
        parse = phase_ms(&r.timings, Phase::Parse),
        total_ms = r.timings.spans.iter().map(|s| s.ms).sum::<f64>(),
    )
}

/// Milliseconds recorded for one phase. `ScanTimings` is an ordered span list
/// (a phase the run skipped — `Classify` with `--no-orientation-cls` — is simply
/// absent), so a missing phase reads as 0.0. Kotlin twin: `Timings.kt`'s `ms()`.
fn phase_ms(t: &ScanTimings, phase: Phase) -> f64 {
    t.spans
        .iter()
        .filter(|s| s.phase == phase)
        .map(|s| s.ms)
        .sum()
}

fn failure_json(name: &str, message: &str) -> String {
    format!(
        r#"    {{
      "name": {},
      "merchant": "",
      "date": null,
      "dateIsPlaceholder": false,
      "total": "",
      "subtotal": null,
      "tax": null,
      "items": [],
      "warnings": [],
      "wallMs": 0.0,
      "timings": null,
      "error": {}
    }}"#,
        json_str(name),
        json_str(message)
    )
}

/// Whether a finding belongs in the harness dump — the host twin of the app's
/// `WarningSeverity >= NOTICE`. Kept exhaustive (no `_` arm) so a new core
/// variant is a compile error here rather than a silent change to what the E2E
/// comparison sees.
fn worth_showing(kind: ReceiptWarningKind) -> bool {
    match kind {
        ReceiptWarningKind::TotalMismatch
        | ReceiptWarningKind::SubtotalMismatch
        | ReceiptWarningKind::PossibleMissedItem
        | ReceiptWarningKind::DroppedImplausiblePrice
        | ReceiptWarningKind::TenderMismatch => true,
        ReceiptWarningKind::PriceAutoCorrected | ReceiptWarningKind::UncategorizedItem => false,
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[allow(dead_code)]
fn _path_exists(p: &Path) -> bool {
    p.exists()
}
