"""Price first-on-mint completing prints on the full tape.

Fill: last print with ts <= fire + lag_ms (fallback: the firing print).
Exit: first-gap (2 slots). Cost: 125 bps/leg + own B/vsol. B = 0.10 SOL.
One episode per mint per book.

Usage: DATABASE_URL in hunter/.env (never printed).
"""
from __future__ import annotations

import sys
from pathlib import Path

import psycopg2

HUNTER_ENV = Path(__file__).resolve().parents[3] / ".env"
SQL_FILE = Path(__file__).with_name("ixg-new-money.sql")


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


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    url = load_url()
    conn = psycopg2.connect(url)
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SET statement_timeout = 0")
    cur.execute("SET work_mem = '2GB'")
    cur.execute("SET synchronous_commit = off")

    stmts = split_sql(SQL_FILE.read_text(encoding="utf-8"))
    print(f"building {len(stmts)} sql steps ...", flush=True)
    for i, stmt in enumerate(stmts, 1):
        head = next(
            (ln.strip()[:80] for ln in stmt.splitlines() if ln.strip() and not ln.strip().startswith("--")),
            stmt[:80],
        )
        print(f"  [{i}/{len(stmts)}] {head}", flush=True)
        cur.execute(stmt)
    print("built", flush=True)
    cur.execute("SELECT fam, working, count(*) FROM ixg.ncand GROUP BY 1, 2 ORDER BY 1, 2")
    print("ncand:")
    for r in cur.fetchall():
        print(f"  fam={r[0]} working={r[1]} n={r[2]}")

    mod = load_walk()
    print("loading tape + events ...", flush=True)
    tape = mod.q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp
        FROM ixg.fall
        WHERE mint IN (SELECT DISTINCT mint FROM ixg.ncand)
          AND px IS NOT NULL AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    ev = mod.q(
        conn,
        """
        SELECT e.mint, e.slot, e.tx_index, e.ts, e.fam, e.working,
               e.this_tmpl, e.vsol_pre, e.created_at,
               EXTRACT(EPOCH FROM (e.ts - e.created_at)) AS age_s,
               (h.mint IS NOT NULL) AS his
        FROM ixg.ncand e
        LEFT JOIN (SELECT DISTINCT mint FROM w8.buys) h ON h.mint = e.mint
        ORDER BY e.mint, e.ts
        """,
    )
    conn.close()
    print(f"tape={len(tape)} ev={len(ev)} mints={ev['mint'].nunique()}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    work = ev["working"].fillna(False).astype(bool)
    books = {
        "solo_new": ev[ev["fam"] == "solo_new"],
        "solo_new_work": ev[(ev["fam"] == "solo_new") & work],
        "same_new": ev[ev["fam"] == "same_new"],
        "same_new_work": ev[(ev["fam"] == "same_new") & work],
        "mixed_new": ev[ev["fam"] == "mixed_new"],
        "solo_rep": ev[ev["fam"] == "solo_rep"],
        "same_rep": ev[ev["fam"] == "same_rep"],
        "mixed_rep": ev[ev["fam"] == "mixed_rep"],
    }

    jobs = []
    for name in ("solo_new", "solo_new_work", "same_new_work", "mixed_new"):
        df = books[name]
        jobs.append((f"{name} gap 0ms first", df, 0, "gap", "first"))
        jobs.append((f"{name} gap 95ms first", df, 95, "gap", "first"))
    jobs.append(("solo_new_work clock 0ms first", books["solo_new_work"], 0, "clock", "first"))
    for name in ("solo_rep", "same_rep", "mixed_rep"):
        jobs.append((f"{name} gap 95ms first", books[name], 95, "gap", "first"))

    snw = books["solo_new_work"]
    snw_his = snw["his"].fillna(False).astype(bool)
    jobs.append(("solo_new_work HIS gap 95ms", snw[snw_his], 95, "gap", "first"))
    jobs.append(("solo_new_work OTHER gap 95ms", snw[~snw_his], 95, "gap", "first"))

    for label, df, lag, mode, pol in jobs:
        print(f"running {label} n={len(df)}...", flush=True)
        if len(df) == 0:
            print(f"{label}: empty")
            continue
        out = mod.run_book(tape_g, df, lag, mode, pol)
        mod.summarize(out, label)


if __name__ == "__main__":
    main()
