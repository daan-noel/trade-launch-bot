//! Offline decoder diagnostics. Two commands, one loop:
//!
//! * `unknown-programs` reads the persisted `trades.ix_labels` and ranks the
//!   program IDs the labeler still cannot name.
//! * `decode-harvest` takes those programs back to the chain and works out what
//!   their instructions are called, then prints paste-ready rows for
//!   `ingest_pumpfun::decode::program_registry`.
//!
//! Neither touches the trading path: both exit before the live bin requires
//! wallet keys, and `decode-harvest` is the only one that spends Helius credits
//! (one `getTransactionsForAddress` per program, not one per transaction).

use anyhow::{anyhow, Context};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

use trading_core::config;
use trading_core::services::helius_rpc::HeliusRpc;

// ── unknown-programs ─────────────────────────────────────────────────────────

/// `unknown-programs [--days N] [--top N]` — offline diagnostic.
///
/// Ranks the program IDs the labeler still can't name — the ones that land as
/// `Unknown (<program id>): <ix>` in the persisted `trades.ix_labels` — by how
/// often they appear, and prints each with a Solscan link. `trades.ix_labels` is
/// the source (raw_txs is not persisted in this deployment), so this reflects
/// real labelled traffic.
///
/// Feed the top of this list to `decode-harvest`, which is what actually names
/// the instructions; look the *program* up on Solscan only when you need the
/// owner's name, and remember that a name is optional — `decode-harvest` can
/// make a program's instructions legible without it.
///
/// NOTE: rows written before the full-id labeler shipped carry the old
/// `Unknown (...suffix)` form and are grouped separately.
///
///   --days N   look back N days over trades (default 3)
///   --top N    print the top N programs (default 40)
pub async fn run_unknown_programs(
    settings: &config::Settings,
    args: Vec<String>,
) -> anyhow::Result<()> {
    let days = arg_i32(&args, "--days", 3);
    let top = arg_i64(&args, "--top", 40);

    let pools = trading_core::storage::postgres::connect(settings).await?;
    tracing::info!(days, top, "unknown-programs: aggregating trades.ix_labels");

    let rows = rank_unknown_programs(&pools.api, days, top).await?;
    let (full, legacy): (Vec<_>, Vec<_>) =
        rows.into_iter().partition(|(id, _)| !id.starts_with("..."));

    println!();
    if full.is_empty() && legacy.is_empty() {
        println!("No `Unknown (...)` labels in the last {days} day(s) — every program is named.");
        return Ok(());
    }

    if full.is_empty() {
        println!(
            "No full-id unknowns yet (only legacy suffixes below) — deploy the full-id\n\
             labeler and let new trades flow, then re-run to get lookup-able program IDs.\n"
        );
    } else {
        println!("Unnamed programs (full id — feed these to `decode-harvest`):\n");
        println!("{:>10}  {:<44}  solscan", "count", "program_id");
        for (id, n) in &full {
            println!("{n:>10}  {id:<44}  https://solscan.io/account/{id}");
        }
    }

    if !legacy.is_empty() {
        println!("\nLegacy 8-char suffixes (pre-full-id rows — not directly lookup-able):");
        for (suffix, n) in &legacy {
            println!("{n:>10}  {suffix}");
        }
    }
    println!(
        "\nNext: cargo run -p hunter-live -- decode-harvest --days {days} --top 20\n\
         A program's NAME is optional; its instruction names are not."
    );
    Ok(())
}

/// `(program_id, occurrences)` for the unnamed programs in the window, busiest
/// first. Shared by both commands so they always rank the same way.
async fn rank_unknown_programs(
    pool: &sqlx::PgPool,
    days: i32,
    top: i64,
) -> anyhow::Result<Vec<(String, i64)>> {
    // Unnest the JSONB label arrays and keep the `Unknown (...)` ones. The label
    // now carries an instruction half (`Unknown (<id>): PumpSwapV3`), so group on
    // the program id alone — otherwise one program ranks once per instruction.
    // Both persisted label shapes (bare array, `{"instructions": [...]}`) are read.
    let rows: Vec<(String, i64)> = sqlx::query_as::<_, (String, i64)>(
        "SELECT split_part(split_part(label, '(', 2), ')', 1) AS program_id, count(*) AS n \
         FROM trades t, LATERAL jsonb_array_elements_text( \
                CASE WHEN jsonb_typeof(t.ix_labels) = 'array' \
                     THEN t.ix_labels ELSE t.ix_labels -> 'instructions' END) AS label \
         WHERE t.ix_labels IS NOT NULL \
           AND t.block_time >= now() - make_interval(days => $1) \
           AND label LIKE 'Unknown (%' \
         GROUP BY 1 \
         ORDER BY n DESC \
         LIMIT $2",
    )
    .bind(days)
    .bind(top)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ── decode-harvest ───────────────────────────────────────────────────────────

/// Transactions sampled per program. One RPC call covers all of them.
const DEFAULT_TXS: usize = 200;

/// `decode-harvest [--days N] [--top N] [--txs N] [--program ID ...]`
///
/// Reads programs off chain and works out what their instructions are called.
///
/// The method, and why its output can be trusted: an Anchor program logs
/// `Program log: Instruction: <Name>` on every invoke. Pair that line with the
/// discriminator of the instruction that produced it and the pair is
/// **checkable** — recompute `sha256("global:<snake(Name)>")[..8]` and it must
/// equal the discriminator we saw. A pair that verifies is proof, not a lookup,
/// and goes to `ANCHOR_IX` as a NAME (the discriminator stays computed, so the
/// row cannot carry a transcription error). A pair that does not verify is a
/// program that dispatches some other way; it is printed separately for
/// `EXPLICIT_IX`, where the key bytes are written down and therefore reviewed.
///
/// Programs that log nothing yield no names — only a key width, which is what
/// decides how the labeler renders them (`ix#<key>`). The command prints the key
/// cardinalities it measured so that choice is visible rather than assumed.
///
///   --days N      window for ranking unknown programs (default 3)
///   --top N       how many unnamed programs to harvest (default 15)
///   --txs N       transactions to sample per program (default 200)
///   --program ID  harvest this program instead of the ranked unknowns
///                 (repeatable; skips the database entirely)
pub async fn run_decode_harvest(
    settings: &config::Settings,
    args: Vec<String>,
) -> anyhow::Result<()> {
    let rpc_url = settings.helius_rpc_url.trim().to_string();
    if rpc_url.is_empty() {
        return Err(anyhow!("decode-harvest needs HELIUS_RPC_URL"));
    }
    let txs = arg_usize(&args, "--txs", DEFAULT_TXS);

    let explicit = arg_all(&args, "--program");
    let programs: Vec<(String, i64)> = if explicit.is_empty() {
        let days = arg_i32(&args, "--days", 3);
        let top = arg_i64(&args, "--top", 15);
        let pools = trading_core::storage::postgres::connect(settings).await?;
        tracing::info!(days, top, "decode-harvest: ranking unnamed programs");
        rank_unknown_programs(&pools.api, days, top)
            .await?
            .into_iter()
            .filter(|(id, _)| !id.starts_with("..."))
            .collect()
    } else {
        explicit.into_iter().map(|id| (id, 0)).collect()
    };

    if programs.is_empty() {
        println!("Nothing to harvest — every program in the window is already named.");
        return Ok(());
    }

    let rpc = HeliusRpc::new(rpc_url);
    let mut memos: BTreeMap<String, usize> = BTreeMap::new();

    println!("\n// decode-harvest — paste-ready rows for program_registry.rs");
    println!("// Sampled {txs} transactions per program.\n");

    for (program_id, seen) in &programs {
        let (page, _) = rpc
            .get_transactions_for_address_full_page_enc(program_id, "desc", txs, None, "jsonParsed")
            .await
            .with_context(|| format!("fetching transactions for {program_id}"))?;

        let report = harvest_program(program_id, &page, &mut memos);
        print_report(program_id, *seen, page.len(), &report);
    }

    print_memos(&memos);
    Ok(())
}

/// What one program's sample says about its instructions.
#[derive(Default)]
struct Harvest {
    /// Log name → snake name, for pairs whose discriminator verified.
    verified: BTreeMap<String, String>,
    /// Log name → observed key hex, for pairs that did NOT verify.
    unverified: BTreeMap<String, String>,
    /// Key hex → occurrences, for instructions that never got a name.
    unnamed: BTreeMap<String, usize>,
    /// Distinct-key counts at each candidate width, which is what picks the width.
    distinct_disc8: usize,
    distinct_tag1: usize,
    distinct_len: usize,
    /// Instructions of this program seen at the top level.
    ix_seen: usize,
}

fn harvest_program(
    program_id: &str,
    page: &[Value],
    memos: &mut BTreeMap<String, usize>,
) -> Harvest {
    let mut h = Harvest::default();
    let mut disc8: HashMap<String, usize> = HashMap::new();
    let mut tag1: HashMap<String, usize> = HashMap::new();
    let mut lens: HashMap<usize, usize> = HashMap::new();
    // Names that resolved, so an instruction is only reported unnamed once the
    // whole sample failed to name it.
    let mut named_keys: HashMap<String, ()> = HashMap::new();
    let mut pairs: Vec<(Vec<u8>, String)> = Vec::new();

    for tx in page {
        let payloads = outer_ix_payloads(tx, program_id);
        h.ix_seen += payloads.len();
        for data in &payloads {
            if data.len() >= 8 {
                *disc8.entry(hex(&data[..8])).or_default() += 1;
            }
            if !data.is_empty() {
                *tag1.entry(hex(&data[..1])).or_default() += 1;
            }
            *lens.entry(data.len()).or_default() += 1;
        }

        for (data, name) in payloads.iter().cloned().zip(logged_ix_names(tx, program_id)) {
            pairs.push((data, name));
        }
        collect_memos(tx, memos);
    }

    for (data, log_name) in &pairs {
        let snake = snake_case(log_name);
        if data.len() >= 8 && anchor_discriminator(&snake) == data[..8] {
            h.verified.insert(log_name.clone(), snake);
            named_keys.insert(hex(&data[..8]), ());
        } else if !data.is_empty() {
            // Not anchor-hashed. Record the narrowest key that still separates
            // it: an 8-byte read of a 1-byte tag would fold the arguments in.
            let key = if data.len() >= 8 && disc8.len() <= tag1.len() {
                hex(&data[..8])
            } else {
                hex(&data[..1])
            };
            h.unverified.insert(log_name.clone(), key.clone());
            named_keys.insert(key, ());
        }
    }

    h.distinct_disc8 = disc8.len();
    h.distinct_tag1 = tag1.len();
    h.distinct_len = lens.len();

    let counted = match pick_key_width(&h) {
        "Tag1" => &tag1,
        _ => &disc8,
    };
    for (key, n) in counted {
        if !named_keys.contains_key(key) {
            h.unnamed.insert(key.clone(), *n);
        }
    }
    h
}

/// The key width the labeler should use for this program.
///
/// Width is a cardinality decision, not a taste one: too wide and a `u64`
/// argument forks one instruction into thousands of labels; too narrow and two
/// instructions merge. The distinct-length count is reported alongside because
/// it is how you tell a genuine multi-instruction program from one instruction
/// carrying an argument.
fn pick_key_width(h: &Harvest) -> &'static str {
    if h.distinct_disc8 > 8 && h.distinct_disc8 > 2 * h.distinct_tag1.max(1) {
        return "Tag1";
    }
    "Disc8"
}

fn print_report(program_id: &str, seen: i64, txs: usize, h: &Harvest) {
    println!("// ── {program_id}");
    println!(
        "//    labelled occurrences in window: {seen} | txs sampled: {txs} | top-level ix: {}",
        h.ix_seen,
    );
    println!(
        "//    distinct keys — disc8 {} / tag1 {} / len {} → key width {}",
        h.distinct_disc8,
        h.distinct_tag1,
        h.distinct_len,
        pick_key_width(h),
    );

    if !h.verified.is_empty() {
        println!("//    ANCHOR_IX (name verified against the observed discriminator):");
        println!("    (\n        \"{program_id}\",\n        &[");
        for (log_name, snake) in &h.verified {
            println!("            (\"{snake}\", \"{log_name}\"),");
        }
        println!("        ],\n    ),");
    }

    if !h.unverified.is_empty() {
        println!(
            "//    EXPLICIT_IX (logs a name but does NOT hash it — keys are transcribed,\n\
             //    so read these before pasting):"
        );
        println!(
            "    (\n        \"{program_id}\",\n        IxKey::{},\n        &[",
            pick_key_width(h),
        );
        for (log_name, key) in &h.unverified {
            println!("            (\"{key}\", \"{log_name}\"),");
        }
        println!("        ],\n    ),");
    }

    if h.verified.is_empty() && h.unverified.is_empty() {
        println!("//    no `Program log: Instruction:` lines — identity only, no names.");
    }

    if !h.unnamed.is_empty() {
        let mut top: Vec<(&String, &usize)> = h.unnamed.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1));
        let listed: Vec<String> =
            top.iter().take(8).map(|(k, n)| format!("ix#{k} x{n}")).collect();
        println!("//    unnamed keys: {}", listed.join(", "));
    }
    println!();
}

fn print_memos(memos: &BTreeMap<String, usize>) {
    if memos.is_empty() {
        return;
    }
    let mut top: Vec<(&String, &usize)> = memos.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1));
    // Memo text is reported HERE and nowhere else: it is per-transaction unique
    // often enough that putting it in `ix_labels` would make `ix_hash` unique per
    // trade. A payload that repeats across transactions is the interesting case,
    // and it is only visible in aggregate.
    println!("// memo payloads seen in the sample (repeated ones are identities):");
    for (text, n) in top.iter().take(12) {
        if **n < 2 {
            continue;
        }
        let shown: String = text.chars().take(60).collect();
        println!("//   {n:>5}  {shown}");
    }
    println!();
}

// ── transaction readers ──────────────────────────────────────────────────────

/// Instruction `data` for every TOP-LEVEL instruction of `program_id`, in order.
fn outer_ix_payloads(tx: &Value, program_id: &str) -> Vec<Vec<u8>> {
    tx.get("transaction")
        .and_then(|t| t.get("message"))
        .and_then(|m| m.get("instructions"))
        .and_then(Value::as_array)
        .map(|ixs| {
            ixs.iter()
                .filter(|ix| ix.get("programId").and_then(Value::as_str) == Some(program_id))
                .filter_map(|ix| ix.get("data").and_then(Value::as_str))
                .filter_map(|d| bs58::decode(d).into_vec().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The instruction names `program_id` logged at the TOP level, in order.
///
/// Only `invoke [1]` counts: a CPI into the same program logs a name too, and
/// pairing that with a top-level instruction would shift every later pair by one.
fn logged_ix_names(tx: &Value, program_id: &str) -> Vec<String> {
    let logs = match tx.get("meta").and_then(|m| m.get("logMessages")).and_then(Value::as_array) {
        Some(l) => l,
        None => return Vec::new(),
    };
    let invoke = format!("Program {program_id} invoke [1]");
    let mut out = Vec::new();
    for (i, line) in logs.iter().enumerate() {
        if line.as_str() != Some(invoke.as_str()) {
            continue;
        }
        // The name is logged by the dispatcher, i.e. immediately — before the
        // program invokes anything else.
        for next in logs.iter().skip(i + 1).take(3) {
            let s = match next.as_str() {
                Some(s) => s,
                None => break,
            };
            if let Some(name) = s.strip_prefix("Program log: Instruction: ") {
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric()) {
                    out.push(name.to_string());
                }
                break;
            }
            if s.starts_with("Program ") && s.contains(" invoke [") {
                break;
            }
        }
    }
    out
}

fn collect_memos(tx: &Value, memos: &mut BTreeMap<String, usize>) {
    let ixs = tx
        .get("transaction")
        .and_then(|t| t.get("message"))
        .and_then(|m| m.get("instructions"))
        .and_then(Value::as_array);
    let Some(ixs) = ixs else { return };
    for ix in ixs {
        if ix.get("program").and_then(Value::as_str) != Some("spl-memo") {
            continue;
        }
        if let Some(text) = ix.get("parsed").and_then(Value::as_str) {
            *memos.entry(text.to_string()).or_default() += 1;
        }
    }
}

// ── naming ───────────────────────────────────────────────────────────────────

/// Anchor logs the PascalCase of the snake_case instruction name; this is that
/// mapping read backwards. It only ever feeds a hash that is then CHECKED, so a
/// mis-conversion downgrades a row from verified to explicit — it cannot produce
/// a wrong name.
fn snake_case(pascal: &str) -> String {
    let mut out = String::with_capacity(pascal.len() + 4);
    for (i, c) in pascal.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `sha256("global:<name>")[..8]` — the same computation `program_registry` uses.
fn anchor_discriminator(name: &str) -> [u8; 8] {
    let digest = solana_sdk::hash::hash(format!("global:{name}").as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest.to_bytes()[..8]);
    out
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

// ── argument parsing ─────────────────────────────────────────────────────────

fn arg_val(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

fn arg_all(args: &[String], flag: &str) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter(|(_, a)| a.as_str() == flag)
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect()
}

fn arg_i32(args: &[String], flag: &str, default: i32) -> i32 {
    arg_val(args, flag).and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn arg_i64(args: &[String], flag: &str, default: i64) -> i64 {
    arg_val(args, flag).and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn arg_usize(args: &[String], flag: &str, default: usize) -> usize {
    arg_val(args, flag).and_then(|s| s.parse().ok()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn snake_case_inverts_anchors_pascal_case() {
        for (pascal, snake) in [
            ("Buy", "buy"),
            ("MultiSwap2", "multi_swap2"),
            ("PumpBuyV2", "pump_buy_v2"),
            ("V2SellExactOutPumpFun", "v2_sell_exact_out_pump_fun"),
            ("SellBondingCurvePercentage", "sell_bonding_curve_percentage"),
        ] {
            assert_eq!(snake_case(pascal), snake, "{pascal}");
        }
    }

    #[test]
    fn a_logged_name_reproduces_the_discriminator_it_was_seen_with() {
        // The pairs below were read off chain. This is the verification step the
        // command applies to every candidate before it prints an ANCHOR_IX row.
        for (log_name, disc_hex) in [
            ("V2BuyExactInPumpFun", "a0bbe397d2052255"),
            ("PumpSwapV3", "af051981a0d8389d"),
            ("FeeTransferWithTip", "4d4df51d1cf91bee"),
            ("MultiSwap2", "8409d42d2771d736"),
        ] {
            assert_eq!(hex(&anchor_discriminator(&snake_case(log_name))), disc_hex);
        }
        // And a name that is NOT how the program hashes: reported, never emitted
        // as an anchor row.
        assert_ne!(hex(&anchor_discriminator(&snake_case("SellPumpfun"))), "4be09fdde54f2eb3");
    }

    fn tx(logs: Vec<&str>, ixs: Vec<Value>) -> Value {
        json!({
            "transaction": {"message": {"instructions": ixs}},
            "meta": {"logMessages": logs},
        })
    }

    #[test]
    fn only_top_level_invocations_are_paired() {
        let pid = "term9YPb9mzAsABaqN71A4xdbxHmpBNZavpBiQKZzN3";
        let t = tx(
            vec![
                &format!("Program {pid} invoke [1]"),
                "Program log: Instruction: RouteOpen",
                "Program 11111111111111111111111111111111 invoke [2]",
                &format!("Program {pid} invoke [2]"),
                // A CPI into the same program logs a name too — pairing it with a
                // top-level instruction would shift every later pair.
                "Program log: Instruction: RouteClose",
            ],
            vec![json!({"programId": pid, "data": "1"})],
        );
        assert_eq!(logged_ix_names(&t, pid), vec!["RouteOpen".to_string()]);
    }

    #[test]
    fn a_program_that_logs_nothing_yields_no_names() {
        let pid = "FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9";
        let t = tx(
            vec![&format!("Program {pid} invoke [1]"), "Program log: GetFees"],
            vec![json!({"programId": pid, "data": "1"})],
        );
        assert!(logged_ix_names(&t, pid).is_empty());
    }

    #[test]
    fn an_argument_carrying_tag_does_not_get_read_as_many_instructions() {
        // Axiom's real shape: one leading tag byte plus a u64 amount. Reading
        // eight bytes forks it into a hundred keys; reading one finds the two
        // instructions that are actually there.
        let axiom = Harvest {
            distinct_disc8: 103,
            distinct_tag1: 2,
            distinct_len: 4,
            ..Default::default()
        };
        assert_eq!(pick_key_width(&axiom), "Tag1");
        // A tag program: the u64 argument forks disc8 but not tag1.
        let tagged = Harvest {
            distinct_disc8: 40,
            distinct_tag1: 6,
            distinct_len: 9,
            ..Default::default()
        };
        assert_eq!(pick_key_width(&tagged), "Tag1");
        // An anchor program that named everything it ran.
        let anchor = Harvest {
            verified: [("Buy".to_string(), "buy".to_string())].into_iter().collect(),
            distinct_disc8: 2,
            distinct_tag1: 2,
            distinct_len: 2,
            ..Default::default()
        };
        assert_eq!(pick_key_width(&anchor), "Disc8");
    }
}
