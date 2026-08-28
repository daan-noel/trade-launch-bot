//! Build-template grain — SQL `tmpl` spelling `program|CU|ATA|N|S|F`.
//!
//! One function, one hash. Not full ordered `ix_hash` and not marker bits: the
//! harvest working list is this grain, and a hash of the grain string is what
//! [`TradeLite::template_hash`](super::TradeLite::template_hash) carries so the
//! fold never rebuilds the string.
//!
//! Guard-tested against the SQL in `ixg-new-money.sql` / `ixg-combined-money.sql`.

use serde_json::Value;

use crate::grouping::normalize_labels;
use crate::hash::fnv1a;

/// Head = first non-boilerplate label. Boilerplate is compute-budget, ATA, token
/// program setup, memos, and the system-program nonce/seed/fee/create-account
/// instructions — the same set `ixg.head` skips.
fn is_boilerplate(label: &str) -> bool {
    label.starts_with("Compute Budget:")
        || label.starts_with("Associated Token:")
        || label.starts_with("Token Program:")
        || label.starts_with("Token 2022:")
        || label.starts_with("Memo Program:")
        || matches!(
            label,
            "System Program: Transfer"
                | "System Program: AdvanceNonceAccount"
                | "System Program: CreateAccountWithSeed"
                | "System Program: CreateAccount"
        )
}

fn has_prefix(labels: &[impl AsRef<str>], pfx: &str) -> bool {
    labels.iter().any(|l| l.as_ref().starts_with(pfx))
}

fn has_label(labels: &[impl AsRef<str>], exact: &str) -> bool {
    labels.iter().any(|l| l.as_ref() == exact)
}

/// Program name — SQL `ixg.program`.
pub fn program_owned(labels: &[impl AsRef<str>]) -> String {
    let head = labels
        .iter()
        .map(|l| l.as_ref())
        .find(|x| !is_boilerplate(x))
        .unwrap_or("(direct)");
    if head == "(direct)" {
        "(direct)".into()
    } else if head.starts_with("Pump.Fun: Create") {
        "launch".into()
    } else if head.starts_with("Pump.Fun:") {
        "Pump.Fun".into()
    } else {
        head.split(':').next().unwrap_or(head).to_string()
    }
}

/// Durable template id — SQL `tmpl`:
/// `program || |CU || |ATA || |N || |S || |F`.
///
/// Empty input still returns `(direct)` (no flags). Callers that mean "missing
/// labels" should not hash this; they set [`TradeLite::template_hash`](super::TradeLite::template_hash)
/// to `None`.
pub fn grain(labels: &[impl AsRef<str>]) -> String {
    let mut s = program_owned(labels);
    if has_prefix(labels, "Compute Budget:") {
        s.push_str("|CU");
    }
    if has_prefix(labels, "Associated Token:") {
        s.push_str("|ATA");
    }
    if has_label(labels, "System Program: AdvanceNonceAccount") {
        s.push_str("|N");
    }
    if has_label(labels, "System Program: CreateAccountWithSeed") {
        s.push_str("|S");
    }
    if has_label(labels, "System Program: Transfer") {
        s.push_str("|F");
    }
    s
}

/// FNV-1a of [`grain`]. `None` on empty/missing labels — same sentinel as
/// [`ix_hash_opt`](super::flow_ix::ix_hash_opt).
pub fn grain_hash(labels: &[impl AsRef<str>]) -> Option<u64> {
    if labels.is_empty() {
        None
    } else {
        Some(fnv1a(grain(labels).as_bytes()))
    }
}

/// FNV-1a of a grain **id string** (`"Axiom Trade|CU|ATA|F"`). The fingerprint
/// working list is stored as these strings; hashing them here means a configured
/// id and a folded trade compare as the same `u64`.
pub fn grain_id_hash(id: &str) -> u64 {
    fnv1a(id.as_bytes())
}

/// [`grain_hash`] over labels already decoded into a [`Value`] — both persisted
/// shapes, via [`normalize_labels`].
pub fn grain_hash_from_labels_value(labels: &Value) -> Option<u64> {
    grain_hash(&normalize_labels(labels))
}

/// [`grain_hash`] over stored JSON text. Falls back to a real parse when the
/// scanner-friendly shape does not apply — same contract as
/// [`ix_hash_from_labels_json`](super::flow_ix::ix_hash_from_labels_json).
pub fn grain_hash_from_labels_json(json: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(json).ok()?;
    grain_hash_from_labels_value(&value)
}

/// True when the creation tx's labels include an Associated Token instruction.
/// Empty labels ⇒ `None` (fail closed — unknown, not "no ATA").
pub fn create_ata_present(labels: &[impl AsRef<str>]) -> Option<u128> {
    if labels.is_empty() {
        return None;
    }
    Some(u128::from(has_prefix(labels, "Associated Token:")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_axiom_cu_ata_f() {
        let labels = [
            "Compute Budget: SetComputeUnitLimit",
            "Compute Budget: SetComputeUnitPrice",
            "Associated Token: CreateIdempotent",
            "Axiom Trade: Buy",
            "System Program: Transfer",
        ];
        assert_eq!(grain(&labels), "Axiom Trade|CU|ATA|F");
    }

    #[test]
    fn working_axiom_cu_ata_n_f() {
        let labels = [
            "Compute Budget: SetComputeUnitLimit",
            "Associated Token: Create",
            "Axiom Trade: Buy",
            "System Program: AdvanceNonceAccount",
            "System Program: Transfer",
        ];
        assert_eq!(grain(&labels), "Axiom Trade|CU|ATA|N|F");
    }

    #[test]
    fn bloom_cu_f_no_ata() {
        let labels = [
            "Compute Budget: SetComputeUnitPrice",
            "Bloom Router: Swap",
            "System Program: Transfer",
        ];
        assert_eq!(grain(&labels), "Bloom Router|CU|F");
    }

    #[test]
    fn bloom_short_name() {
        let labels = [
            "Compute Budget: SetComputeUnitLimit",
            "Bloom: Buy",
            "System Program: Transfer",
        ];
        assert_eq!(grain(&labels), "Bloom|CU|F");
    }

    #[test]
    fn photon_terminal_gmgn() {
        let cu_ata_f = |prog: &str| {
            grain(&[
                "Compute Budget: SetComputeUnitLimit",
                "Associated Token: CreateIdempotent",
                &format!("{prog}: Buy"),
                "System Program: Transfer",
            ])
        };
        assert_eq!(cu_ata_f("Photon"), "Photon|CU|ATA|F");
        assert_eq!(cu_ata_f("Terminal"), "Terminal|CU|ATA|F");
        assert_eq!(cu_ata_f("GMGN Bot"), "GMGN Bot|CU|ATA|F");
        assert_eq!(cu_ata_f("GMGN"), "GMGN|CU|ATA|F");
    }

    #[test]
    fn pumpfun_buy_is_pumpfun_not_launch() {
        let labels = ["Pump.Fun: Buy"];
        assert_eq!(grain(&labels), "Pump.Fun");
    }

    #[test]
    fn create_plus_buy_is_launch() {
        let labels = ["Pump.Fun: CreateIdempotent", "Pump.Fun: Buy"];
        assert_eq!(program_owned(&labels), "launch");
        assert_eq!(grain(&labels), "launch");
    }

    #[test]
    fn empty_is_direct() {
        let empty: [&str; 0] = [];
        assert_eq!(grain(&empty), "(direct)");
        assert!(grain_hash(&empty).is_none());
    }

    #[test]
    fn all_boilerplate_is_direct() {
        let labels = [
            "Compute Budget: SetComputeUnitLimit",
            "Associated Token: Create",
            "System Program: Transfer",
        ];
        assert_eq!(grain(&labels), "(direct)|CU|ATA|F");
    }

    #[test]
    fn grain_id_hash_matches_folded_labels() {
        let labels = [
            "Compute Budget: SetComputeUnitLimit",
            "Associated Token: CreateIdempotent",
            "Axiom Trade: Buy",
            "System Program: Transfer",
        ];
        assert_eq!(grain_hash(&labels).unwrap(), grain_id_hash("Axiom Trade|CU|ATA|F"));
    }

    #[test]
    fn create_ata_none_on_empty_zero_without_one_with() {
        let empty: [&str; 0] = [];
        assert!(create_ata_present(&empty).is_none());
        assert_eq!(create_ata_present(&["Pump.Fun: Create"]), Some(0));
        assert_eq!(
            create_ata_present(&["Associated Token: CreateIdempotent", "Pump.Fun: Create"]),
            Some(1)
        );
    }
}
