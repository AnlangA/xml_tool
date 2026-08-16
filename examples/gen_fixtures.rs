//! Writes the deterministic fixture corpus used by tests and benchmarks.
//!
//! Usage: `cargo run --example gen_fixtures [-- <output-dir>]`
//!
//! The output directory defaults to `test_files/`. Large fixtures land in
//! `test_files/generated/` (git-ignored) and the fidelity corpus in
//! `test_files/fidelity/` (committed).

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("test_files"));
    let dir = std::path::PathBuf::from(dir);

    match xml_tool::fixtures::write_all_fixtures(&dir) {
        Ok(()) => println!("Fixtures written to {}", dir.display()),
        Err(err) => {
            eprintln!("Failed to write fixtures to {}: {err}", dir.display());
            std::process::exit(1);
        }
    }
}
