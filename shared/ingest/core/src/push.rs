//! Optional push feeds carried on the SAME subscription as the transaction
//! stream.
//!
//! Venue-neutral and wire-neutral: the host decides what to do with each update
//! (e.g. bridge block metas into an executor's blockhash cache and nonce account
//! updates into its durable-nonce slots), and a feed that cannot carry them
//! simply never emits the matching [`crate::feed::FeedUpdate`] variants.

/// Host callbacks for the non-transaction updates a subscription can carry.
///
/// Both callbacks run ON THE SUPERVISOR TASK — they must be cheap and
/// non-blocking (parse + store; no I/O, no `.await`).
#[derive(Default)]
pub struct PushHooks {
    /// Extra account pubkeys (base58) to watch. Updates arrive at
    /// [`Self::on_account`]. Empty ⇒ no accounts filter.
    pub watch_accounts: Vec<String>,
    /// Called on every block-meta update with `(slot, blockhash,
    /// block_time_unix_secs)`. `Some` ⇒ a block-meta filter is added to the
    /// subscription.
    ///
    /// `block_time_unix_secs` is the chain's own clock for that slot and is the
    /// ONLY chain-time reference on the stream — a *transaction* frame carries a
    /// slot but no block time, so a venue decoder has nothing but its own receive
    /// clock to stamp. A host measuring feed lag (`now - block_time`) must read it
    /// here. Resolution is **whole seconds**, so a single sample bounds lag rather
    /// than timing it; the distribution over many slots is the usable signal.
    #[allow(clippy::type_complexity)]
    pub on_block_meta: Option<Box<dyn Fn(u64, &str, Option<i64>) + Send + Sync>>,
    /// Called on every watched-account update with `(slot, pubkey_base58,
    /// lamports, account_data)`. `lamports` is the account's balance from the
    /// update — the value carrier for System accounts (e.g. a watched wallet),
    /// whose SOL balance isn't in `data`.
    #[allow(clippy::type_complexity)]
    pub on_account: Option<Box<dyn Fn(u64, &str, u64, &[u8]) + Send + Sync>>,
}

impl PushHooks {
    pub fn wants_blocks_meta(&self) -> bool {
        self.on_block_meta.is_some()
    }

    pub fn account_filter(&self) -> Vec<String> {
        if self.on_account.is_some() {
            self.watch_accounts.clone()
        } else {
            Vec::new()
        }
    }

    /// Whether the push feeds alone justify holding the subscription open.
    ///
    /// Decides what happens when the venue has no accounts to watch (another
    /// feed carries the curve and no pool is tracked yet): the stream stays up
    /// carrying only the watched accounts, or it idles.
    ///
    /// **Block metas deliberately do not count.** They are one metered frame per
    /// slot forever, and a subscription with no transaction filter does not
    /// request them at all (`supervisor::build_subscription`) — so they cannot be
    /// the reason a stream is open. The watched accounts can: they are the host's
    /// own pubkeys, they update only when the bot itself transacts, and they keep
    /// the durable-nonce slots re-armed at feed speed instead of at post-send
    /// poll speed.
    pub fn wants_stream(&self) -> bool {
        !self.account_filter().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the watched accounts hold an otherwise-empty subscription open.
    /// Block metas must NOT: they are one metered frame per slot forever, and a
    /// subscription with no transaction filter does not request them at all — so
    /// holding a stream open for them bills the provider for nothing.
    #[test]
    fn only_watched_accounts_hold_an_empty_venue_stream_open() {
        let none = PushHooks::default();
        assert!(!none.wants_stream());

        let metas = PushHooks {
            on_block_meta: Some(Box::new(|_, _, _| {})),
            ..Default::default()
        };
        assert!(
            !metas.wants_stream(),
            "block metas must never be the reason a stream is open"
        );

        let accounts = PushHooks {
            watch_accounts: vec!["wallet".into()],
            on_account: Some(Box::new(|_, _, _, _| {})),
            ..Default::default()
        };
        assert!(accounts.wants_stream());
    }

    /// An account list without an `on_account` hook (or a hook without accounts)
    /// must not create an accounts filter.
    #[test]
    fn push_hooks_gate_their_filters() {
        let none = PushHooks::default();
        assert!(!none.wants_blocks_meta());
        assert!(none.account_filter().is_empty());

        let accounts_no_hook = PushHooks {
            watch_accounts: vec!["a".into()],
            ..Default::default()
        };
        assert!(accounts_no_hook.account_filter().is_empty());

        let wired = PushHooks {
            watch_accounts: vec!["a".into()],
            on_block_meta: Some(Box::new(|_, _, _| {})),
            on_account: Some(Box::new(|_, _, _, _| {})),
        };
        assert!(wired.wants_blocks_meta());
        assert_eq!(wired.account_filter(), vec!["a".to_string()]);
    }
}
