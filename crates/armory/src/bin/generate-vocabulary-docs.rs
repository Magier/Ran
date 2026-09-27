use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let vocabulary = armory::bundled_vocabulary()?;
    let markdown = armory::render_vocabulary_markdown(&vocabulary);
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("armory crate should be under <workspace>/crates")
        .to_path_buf();
    let output = workspace_root.join("docs/book/src/reference/armory-vocabulary.md");
    std::fs::write(&output, markdown)?;
    println!("wrote {}", output.display());
    Ok(())
}
