"""Concentrate fillable combined-machine events with live facts.

Same events as ix-combined-machine (door + 5-slot gap + vsol<46 +
not-all-repeat + working completing print, crowd or turn; tight packs
out). Facts are taken at the completing print only. His mint list is
not a gate.

Two splits on the same rows:
  habit — P(he1): he buys this mint in S or S+1
  tape  — 95 ms fill, clock-20 harvest net

Conjunctions are pre-committed from earlier habit work (age, vsol band,
gap length, shape). Not an exhaustive search. he1 itself is lookahead
and is reported only as an oracle ceiling.

Usage: DATABASE_URL in hunter/.env (never printed). Needs ixg.cm_cand.
  python ixg-concentrate.py
  python ixg-concentrate.py --skip-build
"""
from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import pandas as pd
import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
SQL_FILE = Path(__file__).with_name("ixg-concentrate.sql")
LAG_MS = 95
DAYS = 12


def load_url() -> str:
    for line in HUNTER_ENV.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing in hunter/.env")


def load_walk():
    import importlib.util

    path = str(Path(__file__).with_name("ixg-honest-exit.py"))
    spec = importlib.util.spec_from_file_location("ixg_honest_exit", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def split_sql(text: str) -> list[str]:
    stmts = []
    buf: list[str] = []
    for line in text.splitlines():
        buf.append(line)
        if line.rstrip().endswith(";"):
            stmt = "\n".join(buf).strip()
            body = [
                ln
                for ln in stmt.splitlines()
                if ln.strip() and not ln.strip().startswith("--")
            ]
            if body:
                stmts.append(stmt)
            buf = []
    return stmts


def walk_all(mod, tape_g, ev, lag_ms, mode):
    """Independent clock/gap walk of every event. Returns net/hold/reason aligned to ev."""
    ev = ev.reset_index(drop=True)
    ev["_i"] = mod.index_events(tape_g, ev)
    n = len(ev)
    nets = np.full(n, np.nan)
    holds = np.full(n, np.nan)
    reasons = np.empty(n, dtype=object)
    done = 0
    for mint, grp in ev.groupby("mint", sort=False):
        t = tape_g.get(mint)
        if t is None:
            done += len(grp)
            continue
        times = t["ts"].to_numpy(dtype="datetime64[ns]")
        slots = t["slot"].to_numpy()
        px = t["px"].to_numpy()
        vsol = t["vsol_lp"].to_numpy(dtype=np.float64)
        is_buy = t["trade_type"].to_numpy() == "buy"
        idxs = grp["_i"].to_numpy()
        tss = pd.to_datetime(grp["ts"], utc=True).dt.tz_localize(None).to_numpy(
            dtype="datetime64[ns]"
        )
        sls = grp["slot"].to_numpy()
        locs = grp.index.to_numpy()
        for j, loc in enumerate(locs):
            idx = int(idxs[j])
            if idx < 0:
                continue
            got = mod.walk_one(
                times, slots, px, vsol, is_buy,
                (idx, tss[j], int(sls[j])),
                lag_ms, mode,
            )
            if got is None:
                continue
            r, hold, reason, _xts = got
            nets[loc] = r
            holds[loc] = hold
            reasons[loc] = reason
        done += len(grp)
        if done % 10000 < len(grp):
            print(f"  walked {done}/{n}", flush=True)
    return nets, holds, reasons


def first_per_mint(df):
    return df.sort_values(["mint", "ts"]).groupby("mint", sort=False, as_index=False).head(1)


def book_stats(df):
    """df must have ts, net. Returns dict."""
    if df is None or len(df) == 0:
        return None
    d = df.dropna(subset=["net"])
    if len(d) == 0:
        return None
    mean = float(d["net"].mean())
    med = float(d["net"].median())
    win = float((d["net"] > 0).mean())
    sol = float(0.10 * d["net"].sum())
    hold = float(d["hold"].median()) if "hold" in d.columns else float("nan")
    days = pd.to_datetime(d["ts"]).dt.date
    by = pd.DataFrame({"d": days, "net": d["net"]}).groupby("d")["net"].mean()
    npos = int((by > 0).sum())
    nd = int(len(by))
    ts = pd.to_datetime(d["ts"])
    oos = d[ts >= "2026-08-18"]
    oos_mean = float(oos["net"].mean()) if len(oos) else float("nan")
    oos_n = int(len(oos))
    return {
        "n": int(len(d)),
        "mints": int(d["mint"].nunique()) if "mint" in d.columns else int(len(d)),
        "mean": mean,
        "med": med,
        "win": win,
        "sol": sol,
        "hold": hold,
        "days": f"{npos}/{nd}",
        "npos": npos,
        "nd": nd,
        "per_day": len(d) / DAYS,
        "oos_n": oos_n,
        "oos_mean": oos_mean,
        "he1": float(d["he1"].mean()) if "he1" in d.columns else float("nan"),
    }


def fmt(st):
    if st is None:
        return "empty"
    return (
        f"n={st['n']} ({st['per_day']:.0f}/d) mean={100*st['mean']:.2f}% "
        f"med={100*st['med']:.2f}% win={100*st['win']:.1f}% sol={st['sol']:.1f} "
        f"days+={st['days']} oos={100*st['oos_mean']:.2f}% he1={100*st['he1']:.2f}%"
    )


def print_split(title, df, key):
    print(f"\n== {title}", flush=True)
    rows = []
    for val, g in df.groupby(key, dropna=False, sort=False):
        he1 = float(g["he1"].mean()) if len(g) else 0.0
        hits = int(g["he1"].sum()) if len(g) else 0
        all_st = book_stats(g)
        first_st = book_stats(first_per_mint(g))
        rows.append((val, len(g), hits, he1, all_st, first_st))
    # stable-ish order: by label string
    rows.sort(key=lambda r: str(r[0]))
    print(
        f"{'band':<16} {'n':>7} {'hits':>5} {'he1%':>6}  "
        f"{'all mean':>9} {'all med':>8}  "
        f"{'1/mint n':>8} {'1/mint mean':>11} {'1/mint med':>10} "
        f"{'days+':>6} {'/d':>4} {'oos':>8}"
    )
    for val, n, hits, he1, a, f in rows:
        if a is None or f is None:
            print(f"{str(val):<16} {n:7d} {hits:5d} {100*he1:5.2f}  empty")
            continue
        print(
            f"{str(val):<16} {n:7d} {hits:5d} {100*he1:5.2f}  "
            f"{100*a['mean']:8.2f}% {100*a['med']:7.2f}%  "
            f"{f['n']:8d} {100*f['mean']:10.2f}% {100*f['med']:9.2f}% "
            f"{f['days']:>6} {f['per_day']:4.0f} {100*f['oos_mean']:7.2f}%"
        )


def band_age(s):
    if pd.isna(s):
        return "unk"
    if s < 20:
        return "lt20"
    if s < 60:
        return "20-60"
    if s < 180:
        return "60-180"
    if s < 600:
        return "180-600"
    return "600+"


def band_vsol(v):
    if pd.isna(v):
        return "unk"
    if v < 33:
        return "lt33"
    if v < 36:
        return "33-36"
    if v < 40:
        return "36-40"
    if v < 46:
        return "40-46"
    return "ge46"


def band_gap(d):
    if pd.isna(d):
        return "unk"
    d = int(d)
    if d < 5:
        return "2-4"
    if d < 10:
        return "5-9"
    if d < 20:
        return "10-19"
    if d < 40:
        return "20-39"
    return "40+"


def band_trail(t):
    if pd.isna(t):
        return "unk"
    if t < 15:
        return "lt15"
    if t < 30:
        return "15-30"
    if t < 60:
        return "30-60"
    return "60+"


def band_sol(x):
    if pd.isna(x):
        return "unk"
    if x < 0.9:
        return "lt0.9"
    if x < 1.5:
        return "0.9-1.5"
    if x < 2.5:
        return "1.5-2.5"
    if x < 4:
        return "2.5-4"
    return "ge4"


def band_init(x):
    if pd.isna(x):
        return "unk"
    if x < 0.2:
        return "lt0.2"
    if x < 1:
        return "0.2-1"
    if x < 2:
        return "1-2"
    if x < 5:
        return "2-5"
    if x < 10:
        return "5-10"
    return "10+"


def band_nwal(n):
    if pd.isna(n):
        return "unk"
    n = int(n)
    if n <= 1:
        return "1"
    if n == 2:
        return "2"
    if n == 3:
        return "3"
    return "4+"


def tmpl_fam(t):
    if t is None or (isinstance(t, float) and pd.isna(t)):
        return "unk"
    s = str(t)
    if s.startswith("Axiom"):
        return "Axiom"
    if s.startswith("Photon"):
        return "Photon"
    if s.startswith("Terminal"):
        return "Terminal"
    if s.startswith("GMGN"):
        return "GMGN"
    if s.startswith("Bloom"):
        return "Bloom"
    return "other"


def placebo(base_first, n_keep, n_draw=20, seed=7):
    """Random same-size first-per-mint draws from the fillable first book."""
    rng = np.random.default_rng(seed)
    n = len(base_first)
    if n_keep <= 0 or n_keep >= n:
        return None
    means = []
    for _ in range(n_draw):
        idx = rng.choice(n, size=n_keep, replace=False)
        means.append(float(base_first.iloc[idx]["net"].mean()))
    arr = np.array(means)
    return float(arr.mean()), float(arr.std()), float(arr.min()), float(arr.max())


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    url = load_url()
    conn = psycopg2.connect(url)
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SET statement_timeout = 0")
    cur.execute("SET work_mem = '64MB'")
    cur.execute("SET max_parallel_workers_per_gather = 0")
    cur.execute("SET synchronous_commit = off")

    cur.execute(
        """
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg' AND c.relkind = 'r' AND c.relname = 'cm_cand'
        """
    )
    if cur.fetchone() is None:
        print("missing ixg.cm_cand — run ixg-combined-money.py first")
        conn.close()
        return

    skip = "--skip-build" in sys.argv
    cur.execute(
        """
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ixg' AND c.relkind = 'r' AND c.relname = 'cm_fact'
        """
    )
    have_fact = cur.fetchone() is not None
    if skip and have_fact:
        print("skip-build: ixg.cm_fact present", flush=True)
    else:
        stmts = split_sql(SQL_FILE.read_text(encoding="utf-8"))
        print(f"building {len(stmts)} sql steps ...", flush=True)
        for i, stmt in enumerate(stmts, 1):
            head = next(
                (
                    ln.strip()[:80]
                    for ln in stmt.splitlines()
                    if ln.strip() and not ln.strip().startswith("--")
                ),
                stmt[:80],
            )
            print(f"  [{i}/{len(stmts)}] {head}", flush=True)
            cur.execute(stmt)
        print("built", flush=True)

    mod = load_walk()
    print("loading tape + facts ...", flush=True)
    tape = mod.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp
        FROM ixg.fall
        WHERE mint IN (SELECT DISTINCT mint FROM ixg.cm_fact)
          AND px IS NOT NULL AND px > 0 AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = mod.q(
        conn,
        """
        SELECT *
        FROM ixg.cm_fact
        ORDER BY mint, ts
        """,
    )
    conn.close()
    print(f"tape={len(tape)} ev={len(ev)} mints={ev['mint'].nunique()}", flush=True)

    for col in (
        "vsol_pre", "age_s", "dslot", "trail", "fam_sol", "run_sol",
        "this_sol", "init_sol", "n_sell_gap", "sol_sell_gap", "run_nwal",
        "nwal_new", "nwal_rep", "fam_n", "rn",
    ):
        if col in ev.columns:
            ev[col] = pd.to_numeric(ev[col], errors="coerce")
    ev["he1"] = ev["he1"].fillna(False).astype(bool)
    ev["he_causal"] = ev["he_causal"].fillna(False).astype(bool)
    ev["cashback"] = ev["cashback"].fillna(False).astype(bool)
    ev["ts"] = pd.to_datetime(ev["ts"], utc=True).dt.tz_localize(None)

    ev["age_b"] = ev["age_s"].map(band_age)
    ev["vsol_b"] = ev["vsol_pre"].map(band_vsol)
    ev["gap_b"] = ev["dslot"].map(band_gap)
    ev["trail_b"] = ev["trail"].map(band_trail)
    ev["runsol_b"] = ev["run_sol"].map(band_sol)
    ev["thissol_b"] = ev["this_sol"].map(band_sol)
    ev["init_b"] = ev["init_sol"].map(band_init)
    ev["nwal_b"] = ev["run_nwal"].map(band_nwal)
    ev["famn_b"] = ev["fam_n"].map(band_nwal)
    ev["tmpl_b"] = ev["this_tmpl"].map(tmpl_fam)
    ev["newmix"] = np.where(ev["nwal_rep"].fillna(0) == 0, "all_new", "mixed")
    ev["gap_sell"] = np.where(ev["n_sell_gap"].fillna(0) > 0, "sells", "quiet")
    ev["cb"] = np.where(ev["cashback"], "cb_on", "cb_off")

    tape["ts"] = pd.to_datetime(tape["ts"], utc=True).dt.tz_localize(None)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    print("walking clock-20 at 95 ms on every fillable event ...", flush=True)
    nets, holds, reasons = walk_all(mod, tape_g, ev, LAG_MS, "clock")
    ev["net"] = nets
    ev["hold"] = holds
    ev["reason"] = reasons
    ok = ev.dropna(subset=["net"])
    print(f"priced {len(ok)}/{len(ev)}", flush=True)

    base_first = first_per_mint(ok)
    print("\n== base (fillable clock-20 95ms)", flush=True)
    print("all events     ", fmt(book_stats(ok)))
    print("first per mint ", fmt(book_stats(base_first)))
    print("he1 oracle all ", fmt(book_stats(ok[ok["he1"]])))
    print("he1 oracle 1/m ", fmt(book_stats(first_per_mint(ok[ok["he1"]]))))
    print("he_causal 1/m  ", fmt(book_stats(first_per_mint(ok[ok["he_causal"]]))))

    print_split("shape", ok, "shape")
    print_split("age", ok, "age_b")
    print_split("vsol_pre", ok, "vsol_b")
    print_split("gap dslot", ok, "gap_b")
    print_split("trail", ok, "trail_b")
    print_split("run_sol", ok, "runsol_b")
    print_split("this_sol", ok, "thissol_b")
    print_split("init_sol (create)", ok, "init_b")
    print_split("cashback (create)", ok, "cb")
    print_split("run_nwal", ok, "nwal_b")
    print_split("fam_n", ok, "famn_b")
    print_split("template family", ok, "tmpl_b")
    print_split("all_new vs mixed", ok, "newmix")
    print_split("gap sells", ok, "gap_sell")

    # trail inside one vs crowd — they mean different things
    print_split("trail | shape=one", ok[ok["shape"] == "one"], "trail_b")
    print_split("trail | shape=separated", ok[ok["shape"] == "separated"], "trail_b")
    print_split("age | shape=separated", ok[ok["shape"] == "separated"], "age_b")
    print_split("vsol | shape=separated", ok[ok["shape"] == "separated"], "vsol_b")

    # Pre-committed conjunctions from earlier habit work. Not mined here.
    age180 = ok["age_s"] < 180
    age60 = (ok["age_s"] >= 20) & (ok["age_s"] < 60)
    vsol_mid = (ok["vsol_pre"] >= 33) & (ok["vsol_pre"] < 40)
    gap10 = ok["dslot"] >= 10
    gap1019 = (ok["dslot"] >= 10) & (ok["dslot"] < 20)
    sep = ok["shape"] == "separated"
    init25 = (ok["init_sol"] >= 2) & (ok["init_sol"] < 5)
    all_new = ok["newmix"] == "all_new"
    axiom = ok["tmpl_b"] == "Axiom"

    age20 = ok["age_s"] >= 20
    age180plus = ok["age_s"] >= 180
    trail15 = ok["trail"] >= 15
    trail30 = ok["trail"] >= 30
    gap20 = ok["dslot"] >= 20
    cb_off = ~ok["cashback"]
    bloom = ok["tmpl_b"] == "Bloom"
    one = ok["shape"] == "one"

    cells = [
        ("age<180", age180),
        ("age 20-60", age60),
        ("vsol [33,40)", vsol_mid),
        ("gap>=10", gap10),
        ("gap 10-19", gap1019),
        ("separated", sep),
        ("init [2,5)", init25),
        ("all_new", all_new),
        ("age<180 & vsol[33,40)", age180 & vsol_mid),
        ("age<180 & separated", age180 & sep),
        ("age<180 & gap>=10", age180 & gap10),
        ("age<180 & vsol[33,40) & separated", age180 & vsol_mid & sep),
        ("age<180 & separated & gap>=10", age180 & sep & gap10),
        ("age<180 & Axiom & separated", age180 & axiom & sep),
        ("age 20-60 & vsol[33,40) & separated", age60 & vsol_mid & sep),
        # Thermometer-directed: he rejects age<20 (he1 0.28%) and that
        # band is the dump. Crowds without a dip dump; solos already
        # require trail>=15. One mechanism: not brand-new, already off peak.
        ("age>=20", age20),
        ("age>=180", age180plus),
        ("trail>=15", trail15),
        ("trail>=30", trail30),
        ("age>=20 & trail>=15", age20 & trail15),
        ("age>=180 & trail>=15", age180plus & trail15),
        ("separated & trail>=15", sep & trail15),
        ("separated & trail>=30", sep & trail30),
        ("one & trail>=30", one & trail30),
        ("age>=20 & gap>=20", age20 & gap20),
        ("age>=180 & gap>=20", age180plus & gap20),
        ("cb_off & age>=180", cb_off & age180plus),
        ("Bloom", bloom),
        ("age>=20 & trail>=15 & gap>=20", age20 & trail15 & gap20),
        ("separated & trail>=15 & age>=20", sep & trail15 & age20),
        ("separated & trail>=15 & age>=180", sep & trail15 & age180plus),
    ]

    print("\n== pre-committed conjunctions (first per mint, clock-20 95ms)", flush=True)
    print(
        f"{'cell':<42} {'n':>6} {'/d':>4} {'he1%':>6} {'mean':>8} {'med':>8} "
        f"{'win':>6} {'sol':>7} {'days+':>6} {'oos':>8}  placebo"
    )
    green = []
    for name, mask in cells:
        sub = first_per_mint(ok[mask])
        st = book_stats(sub)
        if st is None:
            print(f"{name:<42} empty")
            continue
        plc = placebo(base_first, st["n"])
        plc_s = ""
        if plc is not None:
            pmean, pstd, pmin, pmax = plc
            z = (st["mean"] - pmean) / pstd if pstd > 0 else float("nan")
            plc_s = f"z={z:+.2f} rand={100*pmean:.2f}%[{100*pmin:.2f},{100*pmax:.2f}]"
        print(
            f"{name:<42} {st['n']:6d} {st['per_day']:4.0f} {100*st['he1']:5.2f} "
            f"{100*st['mean']:7.2f}% {100*st['med']:7.2f}% {100*st['win']:5.1f}% "
            f"{st['sol']:6.1f} {st['days']:>6} {100*st['oos_mean']:7.2f}%  {plc_s}"
        )
        if st["mean"] > 0 and st["npos"] >= 8:
            green.append((name, st, plc_s))

    print("\n== green cells (mean>0 and days+ >= 8/12)", flush=True)
    if not green:
        print("none")
    else:
        for name, st, plc_s in green:
            print(f"  {name}: {fmt(st)}  {plc_s}")

    confirm = [name for name, st, _ in green]
    extra = [
        "age>=20",
        "age>=180",
        "trail>=15",
        "age>=20 & trail>=15",
        "separated & trail>=15",
        "separated & trail>=30",
        "age>=180 & trail>=15",
        "separated & trail>=15 & age>=20",
    ]
    want = []
    seen = set()
    for name in confirm + extra:
        if name not in seen:
            want.append(name)
            seen.add(name)
    masks = {n: m for n, m in cells}
    print("\n== confirm (run_book first-per-mint, 95 ms)", flush=True)
    jobs = [
        ("clock", "clock"),
        ("harvest_clock", "harvest_clock"),
        ("gap", "gap"),
    ]
    for name in want:
        mask = masks.get(name)
        if mask is None:
            continue
        sub = ok.loc[mask, ["mint", "slot", "tx_index", "ts"]].copy()
        if len(sub) == 0:
            continue
        print(f"\n-- {name} n_ev={len(sub)}", flush=True)
        for label, mode in jobs:
            out = mod.run_book(tape_g, sub, LAG_MS, mode, "first")
            mod.summarize(out, f"{name} {label} 95ms first")


if __name__ == "__main__":
    main()
