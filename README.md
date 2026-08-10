# beanbeaver-mobile-util

Shared build and test assets for the two BeanBeaver phone apps —
[iOS](https://github.com/Endle/beanbeaver-ios) and
[Android](https://github.com/Endle/beanbeaver-android). Both consume this repo as
a git submodule at `shared/`.

| | |
|---|---|
| `scripts/compare-e2e.py` | Grades a batch scan's `batch_out.json` against `<stem>.expected.json` ground truth — fuzzy merchant, exact date/total, per-item description/price/account. |
| `scripts/fetch-models.sh` | Downloads the three PP-OCRv5 ONNX weights from `beanbeaver-core`'s `ocr-models-v1` release. |
| `src/bin/batch_e2e.rs` | Host-side twin of the on-device batch runner: scans a directory of JPEGs through the real core and writes `batch_out.json`. |
| `src/bin/uniffi-bindgen.rs` | UniFFI codegen entry point, run by each app's build script to emit Swift / Kotlin glue. |
| `crates/spend-core/` | The spend/budget arithmetic both apps' spending screens are built on: month bucketing, category grouping, the drill-down. Zero dependencies. |

`crates/` is an ordinary cargo workspace — `cargo test` runs here. The two
`src/bin/*.rs` files are **not** part of it: they are source assets compiled
inside each app's own package, so they build against whatever `bb-receipt-ffi`
tag that app pins. See `CLAUDE.md` for why, and for the charter of what may and
may not live here.

## Use it

```bash
git submodule add https://github.com/Endle/beanbeaver-mobile-util shared
```

```toml
# the consuming app's Cargo.toml
[[bin]]
name = "uniffi-bindgen"
path = "shared/src/bin/uniffi-bindgen.rs"

[[bin]]
name = "batch_e2e"
path = "shared/src/bin/batch_e2e.rs"
```

```bash
./shared/scripts/fetch-models.sh                  # -> ./models/
cargo run --release --bin batch_e2e -- \
  --models models --in-dir "$FIXTURES" --out batch_out.json
python3 shared/scripts/compare-e2e.py \
  --results batch_out.json --manifest "$FIXTURES/manifest.json"
```

MIT — see `LICENSE`.
