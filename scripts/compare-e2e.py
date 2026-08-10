#!/usr/bin/env python3
"""Diff on-device batch results against ground-truth expected.json fixtures.

Consumes the `batch_out.json` produced by the app's `-autoRunBatch` harness
(see ReceiptPipeline.swift / BatchRunner) plus a manifest mapping each result
`name` to its `<stem>.expected.json`, and applies the same assertions as the
desktop pytest suite (test_e2e_receipts.py): fuzzy merchant, exact date/total,
and critical-item description/price/category checks, honoring `known_failures`.

Sim path vs. desktop suite — categories:
  The desktop suite resolves item categories from beanbeaver's PUBLIC rules PLUS
  the private suite's `private_rules.toml`. The shipping app bundles only the
  PUBLIC rules and cannot inject private ones at runtime, so any expected
  category that comes from `private_rules.toml` can't reproduce here. Pass
  `--private-rules <private_rules.toml>` and those items' category assertions are
  tolerated (their expected description contains a private keyword). Description
  and price stay hard-checked, and every PUBLIC-rule category is still enforced —
  so a genuine public-rule category regression still fails. As that debt file
  trends to empty, the sim path tightens automatically.

Usage:
  compare-e2e.py --results batch_out.json --manifest manifest.json \
                 [--private-rules /path/to/private_rules.toml]
"""
import argparse
import json
import sys
from decimal import Decimal, InvalidOperation
from difflib import SequenceMatcher


def norm_merchant(v: str) -> str:
    return "".join(ch for ch in (v or "").upper() if ch.isalnum())


def merchant_matches(expected: str, actual: str, any_of) -> bool:
    e, a = norm_merchant(expected), norm_merchant(actual)
    if not e or not a:
        return False
    if e in a or a in e:
        return True
    for alt in any_of or []:
        n = norm_merchant(alt)
        if n and (n in a or a in n):
            return True
    return SequenceMatcher(None, e, a).ratio() >= 0.85


def dec(v):
    try:
        return Decimal(str(v))
    except (InvalidOperation, TypeError):
        return None


def load_private_keywords(path) -> frozenset:
    """Collect the (upper-cased) keyword set from a private_rules.toml so the sim
    path can tolerate categories the public rules don't produce. Best-effort: a
    missing file or missing TOML parser yields an empty set (nothing tolerated)."""
    try:
        try:
            import tomllib  # Python 3.11+
            with open(path, "rb") as f:
                data = tomllib.load(f)
        except ModuleNotFoundError:  # fall back to a tolerant scan of keyword arrays
            import re
            text = open(path, encoding="utf-8").read()
            data = {"rules": [{"keywords": re.findall(r'"([^"]*)"', block)}
                              for block in re.findall(r"keywords\s*=\s*\[(.*?)\]", text, re.S)]}
    except FileNotFoundError:
        print(f"warning: --private-rules {path} not found; no categories tolerated", file=sys.stderr)
        return frozenset()
    return frozenset(kw.upper() for rule in data.get("rules", []) for kw in rule.get("keywords", []))


def check_merchant(res, exp, tol=frozenset()):
    if "merchant" not in exp:
        return None
    if exp.get("merchant_optional"):
        return True
    return merchant_matches(exp["merchant"], res.get("merchant", ""), exp.get("merchant_any_of"))


def check_date(res, exp, tol=frozenset()):
    if "date" not in exp:
        return None
    return res.get("date") == exp["date"]


def check_total(res, exp, tol=frozenset()):
    if "total" not in exp:
        return None
    return dec(res.get("total")) is not None and dec(res.get("total")) == dec(exp["total"])


def category_tolerated(desc_upper: str, tol) -> bool:
    """True when this item's expected category comes from private_rules (its
    description contains a private keyword), so the public-rules-only app can't
    reproduce it and the category assertion is skipped on the sim path."""
    return any(kw in desc_upper for kw in tol)


def check_items(res, exp, tol=frozenset()):
    crit = exp.get("critical_items")
    if not crit:
        return None
    by_desc = {}
    for it in res.get("items", []):
        by_desc.setdefault((it.get("description") or "").upper(), []).append(it)
    for c in crit:
        pat = c["description"].upper()
        want_price = dec(c["price"])
        matches = [it for d, its in by_desc.items() if pat in d or d in pat for it in its]
        if not matches:
            return False
        prices = [dec(it.get("price")) for it in matches]
        if want_price not in prices:
            return False
        # Fixtures have always spelled this "category" while holding a beancount
        # account string. Core v0.7.0 renamed the *emitted* field to `account`
        # (the old `category` held a classifier key, not necessarily an account),
        # so read the new name and fall back to the old for pre-0.7.0 output.
        want_cat = c.get("account") or c.get("category")
        if want_cat and not c.get("category_optional") and not category_tolerated(pat, tol):
            cats = [
                it.get("account") or it.get("category") or ""
                for it in matches
                if dec(it.get("price")) == want_price
            ]
            if not any(want_cat in cat for cat in cats):
                return False
    return True


CHECKS = [("merchant", check_merchant), ("date", check_date),
          ("total", check_total), ("critical_items", check_items)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--results", required=True)
    ap.add_argument("--manifest", required=True)
    ap.add_argument("--private-rules",
                    help="path to private_rules.toml; item categories whose expected description "
                         "matches one of its keywords are tolerated (the app runs public rules only)")
    args = ap.parse_args()

    tol = load_private_keywords(args.private_rules) if args.private_rules else frozenset()

    results = {r["name"]: r for r in json.load(open(args.results))["results"]}
    manifest = json.load(open(args.manifest))

    rows, total_fail, tol_hits = [], 0, 0
    for name in sorted(manifest):
        exp = json.load(open(manifest[name]))
        res = results.get(name)
        known = set(exp.get("known_failures", []))
        tol_hits += sum(1 for c in (exp.get("critical_items") or [])
                        if (c.get("account") or c.get("category"))
                        and category_tolerated(c["description"].upper(), tol))
        if res is None:
            rows.append((name, "NO RESULT", {}, "—"))
            total_fail += 1
            continue
        if res.get("error"):
            rows.append((name, "SCAN ERROR", {}, res["error"][:40]))
            total_fail += 1
            continue
        outcomes = {}
        case_bad = 0
        for field, fn in CHECKS:
            ok = fn(res, exp, tol)
            if ok is None:
                continue
            if ok:
                outcomes[field] = "PASS" if field not in known else "PASS!"  # unexpected pass
            else:
                outcomes[field] = "pass" if field in known else "FAIL"       # tolerated / real
                if field not in known:
                    case_bad += 1
        verdict = "ok" if case_bad == 0 else f"{case_bad} FAIL"
        total_fail += case_bad
        detail = f"{res.get('merchant','')[:16]} {res.get('date','?')} ${res.get('total','?')} ({len(res.get('items',[]))} items, {int(res.get('wallMs',0))}ms)"
        rows.append((name, verdict, outcomes, detail))

    w = max((len(r[0]) for r in rows), default=10)
    print(f"\n{'case':<{w}}  {'verdict':<8}  {'m/d/t/items':<16}  detail")
    print("-" * (w + 60))
    for name, verdict, outcomes, detail in rows:
        flags = "/".join(outcomes.get(f, "-")[:4] for f, _ in CHECKS)
        print(f"{name:<{w}}  {verdict:<8}  {flags:<16}  {detail}")
    passed = sum(1 for r in rows if r[1] in ("ok",))
    print(f"\n{passed}/{len(rows)} cases fully pass; {total_fail} field assertion(s) failed.")
    if tol:
        print(f"(public-rules-only sim path: tolerated {tol_hits} private-rule "
              f"categorie(s) from {len(tol)} keyword(s) in --private-rules)")
    return 1 if total_fail else 0


if __name__ == "__main__":
    sys.exit(main())
