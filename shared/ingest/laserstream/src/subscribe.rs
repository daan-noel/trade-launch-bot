//! `Subscription` → Yellowstone `SubscribeRequest`.
//!
//! The whole translation from the wire-neutral request the supervisor builds
//! into the one gRPC message that carries it. Pure and unit-tested: nothing here
//! opens a socket.

use std::collections::HashMap;

use ingest_core::config::Commitment;
use ingest_core::feed::Subscription;
use ingest_core::proto::geyser::{
    CommitmentLevel, SubscribeRequest, SubscribeRequestFilterAccounts,
    SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterTransactions,
};

pub fn commitment_level(c: Commitment) -> CommitmentLevel {
    match c {
        Commitment::Processed => CommitmentLevel::Processed,
        Commitment::Confirmed => CommitmentLevel::Confirmed,
        Commitment::Finalized => CommitmentLevel::Finalized,
    }
}

/// Build a `Subscribe` request. `filter_key` is the transaction filter-map key
/// (a label the venue owns). `blocks_meta` adds a block-meta filter;
/// `watch_accounts` (non-empty) adds an `accounts` filter — both feed the
/// optional push hooks.
pub fn build_subscribe_request(
    filter_key: &str,
    account_include: Vec<String>,
    from_slot: Option<u64>,
    commitment: CommitmentLevel,
    blocks_meta: bool,
    watch_accounts: Vec<String>,
) -> SubscribeRequest {
    // An empty `account_include` is not "no transactions" to Yellowstone — it is
    // a filter that matches EVERY transaction on chain. Omit the filter entirely
    // instead, which is what "watch no transactions" actually means. This is the
    // shape used when another feed carries the curve and no pool is tracked yet:
    // the subscription still exists to carry the push feeds below.
    let mut transactions = HashMap::new();
    if !account_include.is_empty() {
        transactions.insert(
            filter_key.to_string(),
            SubscribeRequestFilterTransactions {
                vote: Some(false),
                failed: Some(false),
                signature: None,
                account_include,
                account_exclude: Vec::new(),
                account_required: Vec::new(),
            },
        );
    }
    let mut req = SubscribeRequest {
        transactions,
        commitment: Some(commitment as i32),
        from_slot,
        ..Default::default()
    };
    if blocks_meta {
        req.blocks_meta
            .insert(filter_key.to_string(), SubscribeRequestFilterBlocksMeta {});
    }
    if !watch_accounts.is_empty() {
        req.accounts.insert(
            filter_key.to_string(),
            SubscribeRequestFilterAccounts {
                account: watch_accounts,
                owner: Vec::new(),
                filters: Vec::new(),
            },
        );
    }
    req
}

/// The neutral [`Subscription`] the supervisor hands down, as gRPC asks for it.
pub fn request_of(sub: Subscription) -> SubscribeRequest {
    build_subscribe_request(
        sub.filter_key,
        sub.account_include,
        sub.from_slot,
        commitment_level(sub.commitment),
        sub.blocks_meta,
        sub.watch_accounts,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ingest_core::feed::StreamScope;

    /// The filter-map key is venue-supplied (was a hardcoded `"pumpfun"`); a
    /// venue can subscribe under any key without a transport change.
    #[test]
    fn build_subscribe_request_honors_venue_filter_key() {
        let req = build_subscribe_request(
            "myvenue",
            vec!["acct".to_string()],
            Some(42),
            CommitmentLevel::Processed,
            false,
            Vec::new(),
        );
        assert!(req.transactions.contains_key("myvenue"));
        assert_eq!(req.from_slot, Some(42));
        assert_eq!(
            req.transactions["myvenue"].account_include,
            vec!["acct".to_string()]
        );
        // No push hooks → no extra filters (a push-less host's subscription is
        // byte-identical to the pre-push one).
        assert!(req.blocks_meta.is_empty());
        assert!(req.accounts.is_empty());
    }

    /// An empty `account_include` means "watch nothing", but Yellowstone reads it
    /// as "watch everything" — so the filter must be omitted, not sent empty.
    #[test]
    fn an_empty_account_set_omits_the_transactions_filter() {
        let req = build_subscribe_request(
            "pumpfun",
            Vec::new(),
            None,
            CommitmentLevel::Processed,
            true,
            Vec::new(),
        );
        assert!(
            req.transactions.is_empty(),
            "empty account_include must not send a tx filter"
        );
        // This layer only translates: `blocks_meta` is whatever it was handed.
        // Whether to ASK for block metas on a transaction-less subscription is
        // the supervisor's call, and it says no — see
        // `supervisor::build_subscription`.
        assert!(req.blocks_meta.contains_key("pumpfun"));

        let req = build_subscribe_request(
            "pumpfun",
            vec!["pool".into()],
            None,
            CommitmentLevel::Processed,
            false,
            Vec::new(),
        );
        assert_eq!(
            req.transactions["pumpfun"].account_include,
            vec!["pool".to_string()]
        );
    }

    /// Push hooks ride the SAME subscription: `blocks_meta` + `accounts` filters
    /// appear only when the corresponding hook is set.
    #[test]
    fn build_subscribe_request_adds_push_filters() {
        let req = build_subscribe_request(
            "myvenue",
            Vec::new(),
            None,
            CommitmentLevel::Processed,
            true,
            vec!["nonce1".to_string(), "nonce2".to_string()],
        );
        assert!(req.blocks_meta.contains_key("myvenue"));
        assert_eq!(
            req.accounts["myvenue"].account,
            vec!["nonce1".to_string(), "nonce2".to_string()]
        );
    }

    /// The neutral request the supervisor builds survives the translation whole
    /// — this is the only place a `Subscription` becomes gRPC.
    #[test]
    fn a_neutral_subscription_translates_field_for_field() {
        let _ = StreamScope::ALL;
        let req = request_of(Subscription {
            filter_key: "pumpfun",
            account_include: vec!["a".into(), "b".into()],
            from_slot: Some(7),
            commitment: Commitment::Confirmed,
            blocks_meta: true,
            watch_accounts: vec!["w".into()],
        });
        assert_eq!(req.from_slot, Some(7));
        assert_eq!(req.commitment, Some(CommitmentLevel::Confirmed as i32));
        assert_eq!(req.transactions["pumpfun"].account_include.len(), 2);
        assert!(req.blocks_meta.contains_key("pumpfun"));
        assert_eq!(req.accounts["pumpfun"].account, vec!["w".to_string()]);
    }
}
