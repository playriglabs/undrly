// `sqlx::migrate!` embeds `database/migrations/` at compile time; rebuild
// when a migration is added or changed, so no binary runs a stale set.
fn main() {
    println!("cargo:rerun-if-changed=../../../database/migrations");
}
