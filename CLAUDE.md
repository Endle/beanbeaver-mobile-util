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

### Why a separate repo, and what was rejected

The planning doc this repo grew out of has been deleted now that the work is
done, so the decisions worth not relitigating live here:

- **Swift and Kotlin share nothing at source level.** Every "can we share this?"
  answer therefore routes through Rust behind the UniFFI seam both apps already
  consume, or it is a no. "Put it in a shared Swift/Kotlin file" is not an
  option that exists, and that framing is the part people re-derive each time.
- **A sibling crate inside `beanbeaver-core` was considered and rejected.** It
  would be cheaper at release time — no extra pinning hop — but it widens what
  core is for, against the standing charter that core is *solely* bbox →
  itemized JSON. A separate repo keeps that charter literal. The price is one
  more hop in every release, accepted knowingly.
- **Never pass a whole receipt record across the FFI.** The app-side record
  carries the full parse result including `rawText` and `beancount`, and both
  apps recompute the summary on every render, so that would copy every OCR dump
  per frame. `SpendInput` is a deliberately slim projection; mapping into it is
  about ten lines per platform, and it insulates the spend layer from changes to
  the parse result's shape.
- **Not shared, deliberately:** UI; `WarningSeverity` (the parser reports a
  `kind` and each client ranks it — a single shared severity would undo the
  point); `GitHubLedger` (the *policy* is shared but the transport isn't, and
  async HTTP over UniFFI means callback interfaces); and standalone display
  formatters (an FFI hop per formatted price is ceremony for no gain —
  `price_value` and `leaf_label` are here only because the arithmetic needs
  them).

**One thing was never finished:** whether the `SpendInput` projection is cheap
enough to rebuild per render, or whether each app should memoise the summary on
its record list's identity (Android `remember(records, monthId)`, iOS the
store's revision). Both apps currently rebuild and cross the FFI every render —
inherited behaviour, not a regression, but unmeasured. Measure from the app
side, not host Rust, which skips the real serialization.

## Layout

| Path | What | Notes |
|---|---|---|
| `scripts/compare-e2e.py` | Grades `batch_out.json` against `<stem>.expected.json` fixtures | Was byte-identical in both apps. Pure JSON in, verdict out — no path or platform assumptions. |
| `scripts/fetch-models.sh` | Downloads the 3 PP-OCRv5 ONNX weights | Writes to `$MODELS_DIR`, default `$PWD/models` — i.e. the **consuming repo's** root, not this submodule. |
| `src/bin/uniffi-bindgen.rs` | UniFFI codegen entry point (library mode) | Compiled *into* each app's build-only crate via `[[bin]] path = "shared/src/bin/…"`. |
| `src/bin/batch_e2e.rs` | Host-side twin of the on-device `BatchRunner` | Same. Produces the `batch_out.json` that `compare-e2e.py` grades. |
| `crates/spend-core/` | **Phase 2a.** The spend/budget arithmetic — a real cargo crate, unlike the two bins above. Zero dependencies, `cargo test` anywhere. |
| `crates/mobile-ffi/` | **Phase 2b.** The UniFFI seam over `spend-core`, and **the single library both apps link** — it carries two namespaces. See below. |

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

`crates/spend-core` and `crates/mobile-ffi` are ordinary workspace members,
built by `cargo test` / `cargo build` here. The distinction matters: a change to `spend-core` is proven in this repo,
while a change to `src/bin/*.rs` can only be proven in an app.

This makes the repo root a **workspace root nested inside each app's package
directory** (`beanbeaver-android/shared/Cargo.toml`). That is fine — cargo does
not scan subdirectories for packages, so the app's build never sees it.
*Verified*, not assumed: `cargo check` on android's two bins with this workspace
present in `shared/` is unaffected. Re-check it if the layout changes.

## `mobile-ffi` carries two namespaces, and two things keep it that way

`bb-mobile-ffi` depends on `bb-receipt-ffi` **solely so that crate's uniffi
scaffolding lands in the same artifact**. Nothing here calls it. The result is
one `libbb_mobile_ffi.{so,a,dylib}` exposing both `bb_mobile_ffi` and
`bb_receipt_ffi`, so each app pins only this repo and runs one codegen step.

Both of the following were established by measurement. Both fail *silently*.

**1. `use bb_receipt_ffi as _;` in `crates/mobile-ffi/src/lib.rs` is load-bearing.**
An unreferenced dependency is not linked into a `cdylib`, and uniffi's
scaffolding is `#[no_mangle]` statics with nothing referencing them. Delete that
line and everything still builds and every Rust test still passes — the library
just drops from **~59 MB to ~1.7 MB** and bindgen emits **one** namespace, with
no error anywhere. The first symptom is a missing-symbol failure in an app.
CI asserts both namespaces, and that assertion has been checked against the
actual regression, not just written.

**2. Every type in `mobile-ffi` is prefixed `Spend`.** In Swift both namespaces
are generated into one module, so a type name shared with `bb-receipt-ffi` is a
redeclaration error. Core exports `ItemTag`, `ReceiptItem`, `Phase`,
`ScanTimings` and ~20 more, and nothing starting `Spend`. An unprefixed type
here breaks the **iOS** build and only the iOS build — Kotlin puts each
namespace in its own package and would not notice.

No `uniffi.toml` and no `cdylib_name`: in `--library` mode uniffi stamps the
scanned artifact's name into every namespace it emits, so both generated loaders
already resolve `bb_mobile_ffi`.

### Generating bindings here

```bash
cargo build
cargo run --bin uniffi-bindgen -- generate \
  --library target/debug/libbb_mobile_ffi.dylib \
  --language kotlin --out-dir gen        # or --language swift
```

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
