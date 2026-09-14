// `sqlx::migrate!("../migrations")` embeds the migration set at compile time, so a
// new migration file must invalidate this crate's build; otherwise an incremental
// build keeps the old set and the new version never runs.
fn main() {
    println!("cargo:rerun-if-changed=../migrations");
}
