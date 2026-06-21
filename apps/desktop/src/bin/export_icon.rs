#[path = "../icon.rs"]
mod icon;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let assets_dir = root.join("assets");
    std::fs::create_dir_all(&assets_dir)?;
    icon::export_icon_png(assets_dir.join("freedb-icon-1024.png"), 1024)?;
    icon::export_icon_png(assets_dir.join("freedb-icon-256.png"), 256)?;
    Ok(())
}
