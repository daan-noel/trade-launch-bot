//! Cross-crate guard: every tip account the executor SENDS to must be one the
//! decoder RECOGNISES.
//!
//! The two lists are deliberately different sizes and answer different questions.
//! `pump-trader` names the accounts we are allowed to pay — Jito's for block-engine
//! bundles, Helius Sender's for the single-tx path — and paying the wrong one gets a
//! transaction rejected pre-broadcast. `ingest-pumpfun` names every account whose
//! arrival lamports count as somebody's tip while reading the whole tape, so it is a
//! superset by design and grows whenever a new rail carries order flow.
//!
//! Only one relation must hold, and it holds in one direction: **subset**. If an
//! account we send to is missing from the read list, our own tips decode as
//! `tip_lamports = 0` — indistinguishable from a router paying its own rake — and
//! our transactions become the one cohort the fee-budget columns misreport. The
//! reverse is not a defect: recognising a rail we never pay is the whole point.
//!
//! `live` is the only crate that depends on both, so the check lives here. No DB and
//! no network: it runs on a plain `cargo test -p hunter-live`.

use ingest_pumpfun::protocol::Protocol;
use pump_trader::protocol as pt;

#[test]
fn every_tip_account_we_send_to_is_one_the_decoder_recognises() {
    let p = Protocol::pump_fun();

    let sent = pt::JITO_TIP_ACCOUNTS
        .iter()
        .map(|k| ("Jito", k))
        .chain(pt::HELIUS_SENDER_TIP_ACCOUNTS.iter().map(|k| ("Helius Sender", k)));

    for (rail, key) in sent {
        assert!(
            p.is_tip_account(key.as_ref()),
            "{rail} tip account {key} is missing from TIP_ACCOUNT_IDS in \
             shared/ingest/pumpfun/src/protocol.rs — our own tips would decode as \
             tip_lamports = 0"
        );
    }
}

/// A tip account that is not a tip account must not read as one. Guards a paste
/// error in the read list against the only thing it can be checked against here:
/// an address the executor uses for something else entirely.
#[test]
fn an_ordinary_program_address_is_not_a_tip_account() {
    let p = Protocol::pump_fun();
    assert!(!p.is_tip_account(pt::PUMP_FUN.as_ref()));
    assert!(!p.is_tip_account(pt::WSOL_MINT.as_ref()));
}
