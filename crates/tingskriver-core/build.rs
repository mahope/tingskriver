// Rebuild when a migration is added: `sqlx::migrate!` embeds the folder at
// compile time but does not tell Cargo to watch it.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
