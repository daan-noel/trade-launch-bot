//! Strategy-agnostic analysis-scan helper shared by the tpsl matched / simulate
//! endpoints. It streams the **whole** `tokens` table (decoupled from the live,
//! aggressively-evicted `token_cache`) one keyset page at a time and keeps only
//! the tokens a caller-supplied predicate accepts — so an old, evicted-but-matching
//! mint is still considered, while the (windowed) table is never fully resident.
//!
//! Not strategy logic: tpsl1 and tpsl2 stay independent clones and each pass their
//! own `keep` closure (`token_matches_buy_rule` / `token_criteria_satisfied`). Only
//! this scan loop and the [`TokenRepo::find_page_before`] keyset primitive are shared.

use chrono::{DateTime, Utc};

use backend_core::config::constants::ANALYSIS_SCAN_PAGE;
use backend_core::models::token::Token;
use backend_core::storage::repositories::token_repo::TokenRepo;

/// Stream the `tokens` table in keyset pages within the optional `[since, until)`
/// creation-time window (both `None` = all-time), returning the tokens that pass
/// `keep`. Only matches are retained: the match filter is sparse, so the resident
/// set is the match count, not the table size. The page heap (`ANALYSIS_SCAN_PAGE`
/// rows × `Token`) is transient — dropped once each page is filtered.
pub async fn collect_matching_tokens(
    repo: &TokenRepo,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    keep: impl Fn(&Token) -> bool,
) -> anyhow::Result<Vec<Token>> {
    let mut matches: Vec<Token> = Vec::new();
    let mut cursor: Option<(DateTime<Utc>, String)> = None;

    loop {
        let page = repo
            .find_page_before(cursor.clone(), ANALYSIS_SCAN_PAGE, since, until)
            .await?;
        let page_len = page.len();

        // Advance the cursor to the page's last (oldest) row before filtering, so
        // the next page resumes exactly after it regardless of how many matched.
        if let Some(last) = page.last() {
            cursor = Some((last.created_at, last.mint_address.clone()));
        }

        matches.extend(page.into_iter().filter(|t| keep(t)));

        // A short page means we've reached the end of the (windowed) table.
        if (page_len as i64) < ANALYSIS_SCAN_PAGE {
            break;
        }
    }

    Ok(matches)
}
