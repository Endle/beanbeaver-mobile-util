# beanbeaver-mobile-util

The parts [`beanbeaver-ios`](https://github.com/Endle/beanbeaver-ios) and
[`beanbeaver-android`](https://github.com/Endle/beanbeaver-android) genuinely
share. Consumed by both as a **git submodule at `shared/`**.

MIT, like core/ios/android. Nothing here may pull in beancount or anything that
links it — the umbrella `~/src/bb/CLAUDE.md` owns that license firewall, and this
repo sits entirely on the permissive side of it.

## Charter

**In scope:** portable non-UI logic and shared build/test assets for the two
phone apps.

**Out of scope, deliberately:**

- **No UI.** Compose and SwiftUI stay independent, by standing instruction.
- **No platform storage.** `UserDefaults` / `SharedPreferences`, files, photos —
  each app owns its own.
- **No OCR.** PP-OCRv5 lives in `beanbeaver-core`'s `ocr-paddle` and stays there.
  Moving it was considered and rejected: the iOS/Android difference is ORT *link*
  plumbing in `build-xcframework.sh` / `build-android.sh`, which moving the crate
  would not touch. If it ever moves, the home is a standalone `beanbeaver-ocr`.
- **No parsing.** bbox → itemized JSON is `beanbeaver-core`'s charter, verbatim.
- **The two build scripts are not unified.** `build-xcframework.sh` and
  `build-android.sh` look parallel, but the ORT handling genuinely differs (a
  static `.a` folded into an xcframework vs a `.so` in `jniLibs/`). That is the
  one place a false abstraction would cost real debugging time — see the iOS CI
  ORT-cache coupling incident (ios #57).

The full rationale, including the rejected alternatives, is
`~/src/bb/beanbeaver_mobile_util_plan.md`.

## Layout

| Path | What | Notes |
|---|---|---|
| `scripts/compare-e2e.py` | Grades `batch_out.json` against `<stem>.expected.json` fixtures | Was byte-identical in both apps. Pure JSON in, verdict out — no path or platform assumptions. |
| `scripts/fetch-models.sh` | Downloads the 3 PP-OCRv5 ONNX weights | Writes to `$MODELS_DIR`, default `$PWD/models` — i.e. the **consuming repo's** root, not this submodule. |
| `src/bin/uniffi-bindgen.rs` | UniFFI codegen entry point (library mode) | Compiled *into* each app's build-only crate via `[[bin]] path = "shared/src/bin/…"`. |
| `src/bin/batch_e2e.rs` | Host-side twin of the on-device `BatchRunner` | Same. Produces the `batch_out.json` that `compare-e2e.py` grades. |
| `crates/spend-core/` | **Phase 2a.** The spend/budget arithmetic — a real cargo crate, unlike the two bins above. Zero dependencies, `cargo test` anywhere. |

**`models/` is deliberately not here.** The weights are large binaries and the
current arrangement works: iOS commits them, Android fetches them from the
`ocr-models-v1` release on `beanbeaver-core`.

## Two kinds of Rust live here, and they are built differently

There is a cargo workspace at the root (`crates/`), but **`src/bin/*.rs` is not
part of it** — nothing in this repo builds those two files. They are compiled
inside each consuming app's own package, via explicit `[[bin]]` entries:

```toml
[[bin]]
name = "batch_e2e"
path = "shared/src/bin/batch_e2e.rs"
```

That is the whole point, and it is why this is a submodule rather than a cargo
git dependency:

- **`batch_e2e.rs` is core-version-sensitive.** It imports `OcrSession`, `Phase`,
  `ReceiptWarningKind` and `ScanTimings` from `bb-receipt-ffi`, so a breaking FFI
  bump changes it. It has silently rotted before (against v0.6.x's `scan()` arity).
  Compiling it inside each app means it builds against **that app's** pinned core
  tag, so iOS and Android can sit on different tags without this repo having to
  pick one.
- **`uniffi-bindgen.rs`** must be compiled against the same `uniffi` version the
  app's `bb-receipt-ffi` pulls in (0.28). Same reasoning.

So: **when a breaking core FFI change lands, `batch_e2e.rs` may need updating
here**, and each app picks it up by moving its submodule pointer. Check with
`git -C ../beanbeaver-core diff <from> <to> -- crates/ffi/src/lib.rs`; empty
output means no change is needed.

### The crates, which *are* a workspace

`crates/spend-core` (Phase 2a, landed) and `crates/mobile-ffi` (Phase 2b, not
written) are ordinary workspace members, built by `cargo test` / `cargo build`
here. The distinction matters: a change to `spend-core` is proven in this repo,
while a change to `src/bin/*.rs` can only be proven in an app.

This makes the repo root a **workspace root nested inside each app's package
directory** (`beanbeaver-android/shared/Cargo.toml`). That is fine — cargo does
not scan subdirectories for packages, so the app's build never sees it.
*Verified*, not assumed: `cargo check` on android's two bins with this workspace
present in `shared/` is unaffected. Re-check it if the layout changes.

## Working on this repo

`crates/` is testable here and must stay green:

```bash
cargo test && cargo clippy --all-targets && cargo fmt --check
```

A change to `src/bin/*.rs` is a different matter — nothing here compiles those,
so it is not done until **both** apps build against it:

```bash
# in beanbeaver-android
cargo check --bin batch_e2e --bin uniffi-bindgen
./build-android.sh && ./gradlew :app:assembleFullDebug

# in beanbeaver-ios
cargo check --bin batch_e2e --bin uniffi-bindgen
./build-xcframework.sh
```

To move an app onto a new commit here:

```bash
cd shared && git pull origin main && cd ..
git add shared && git commit -m "chore(shared): bump beanbeaver-mobile-util"
```

Both apps' CI checks out with `submodules: true`. A contributor cloning either
app needs `--recurse-submodules` (or `git submodule update --init`); without it
`shared/` is an empty directory and the cargo build fails on a missing bin path.
