//! Generate (or verify) the committed `phast.toml` JSON Schema.
//!
//! Built only with the `schema` feature. `make schema` writes the file;
//! `make schema-check` runs `--check` in CI to fail when the committed copy
//! has drifted from the types.

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let schema = phast_core::schema::generate()?;

    match check_mode() {
        true => verify(&schema),
        false => write(&schema),
    }
}

/// Whether `--check` was passed: verify instead of write.
fn check_mode() -> bool {
    std::env::args().any(|arg| arg == "--check")
}

/// Verify the current schema still matches the freshly generated one, erroring on drift.
///
/// Trailing whitespace is ignored on both sides, because the committed file carries a final
/// newline that the generated string does not.
fn verify(schema: &str) -> anyhow::Result<()> {
    let path = output_path();

    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current.trim_end() != schema.trim_end() {
        anyhow::bail!(
            "{} is out of date; run `make schema` to regenerate",
            path.display(),
        );
    }

    println!("{} is up to date", path.display());
    Ok(())
}

/// Write the schema to its committed path, creating the version directory if needed.
fn write(schema: &str) -> anyhow::Result<()> {
    let path = output_path();

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }

    std::fs::write(&path, format!("{schema}\n"))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The committed schema path, at `schema/v{version}/manifest.json` under the workspace root.
fn output_path() -> PathBuf {
    let version = phast_core::meta::SCHEMA_VERSION;
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../schema/v{version}/manifest.json"))
}
