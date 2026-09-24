// `sqlx::migrate!()` embeds migrations at compile time, and cargo does not
// know that: a new file in migrations/ left the binary without it, so the
// server started "migrated" while missing the newest schema change.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
