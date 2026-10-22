use crate::error::CliError;
use resvg::usvg;
use std::path::Path;
use tiny_skia::{Pixmap, Transform};

/// The seal colour of `kani-mark.svg`, used to bleed the maskable icon to its
/// edges. Keep in sync with the `<rect>` fill in the mark.
fn seal_bleed() -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(0xb9, 0x3a, 0x24, 0xff)
}

pub fn run() -> Result<(), CliError> {
    let svg_path = Path::new("static/icons/kani-mark.svg");
    let out_dir = Path::new("static/icons");
    // `static/icons/` is gitignored, so a clean checkout (CI, Docker, a fresh clone) has no source
    // SVG. Icon generation is polish, not a build step — hard-failing here broke `kani-cli setup`,
    // and with it the Docker image build, for anyone who did not already have the file locally.
    if !svg_path.exists() {
        eprintln!(
            "note: {} not found — skipping PWA icon generation. \
             Add the source SVG to generate icon-192/512/512-maskable.",
            svg_path.display()
        );
        return Ok(());
    }
    generate_icons(svg_path, out_dir)
}

/// Renders `svg_path` to the three PWA icon sizes required by the web app manifest.
///
/// Outputs written to `out_dir`:
/// - `icon-192.png`          — 192×192, standard
/// - `icon-512.png`          — 512×512, standard
/// - `icon-512-maskable.png` — 512×512 with 20% safe-zone padding for maskable use
pub(crate) fn generate_icons(svg_path: &Path, out_dir: &Path) -> Result<(), CliError> {
    let svg_data = std::fs::read(svg_path)
        .map_err(|e| CliError::Other(format!("failed to read {}: {e}", svg_path.display())))?;

    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(&svg_data, &opt)
        .map_err(|e| CliError::Other(format!("failed to parse SVG: {e}")))?;

    std::fs::create_dir_all(out_dir)?;

    render_at_size(&tree, 192, out_dir.join("icon-192.png"), 0.0, None)?;
    render_at_size(&tree, 512, out_dir.join("icon-512.png"), 0.0, None)?;
    // A maskable icon must be full-bleed: the launcher crops it to a shape of its choosing, so
    // transparent padding shows through as clipped corners. Fill the canvas with the mark's seal
    // colour and let the safe zone hold the artwork.
    render_at_size(
        &tree,
        512,
        out_dir.join("icon-512-maskable.png"),
        0.10,
        Some(seal_bleed()),
    )?;

    println!("Icons written to {}", out_dir.display());
    Ok(())
}

/// Renders the tree into a square PNG of `size` × `size` pixels.
///
/// `safe_zone_ratio` adds padding on each side as a fraction of `size`
/// (e.g. 0.10 = 10% per side, leaving 80% for the mark — satisfying the 20% total
/// safe-zone requirement for maskable icons). `bleed` fills the whole canvas
/// first, which a maskable icon needs so its padding is not transparent.
fn render_at_size(
    tree: &usvg::Tree,
    size: u32,
    output: impl AsRef<Path>,
    safe_zone_ratio: f32,
    bleed: Option<tiny_skia::Color>,
) -> Result<(), CliError> {
    let mut pixmap = Pixmap::new(size, size)
        .ok_or_else(|| CliError::Other(format!("failed to allocate {size}×{size} pixmap")))?;
    if let Some(colour) = bleed {
        pixmap.fill(colour);
    }

    let padding = (size as f32 * safe_zone_ratio).round();
    let inner = size as f32 - padding * 2.0;
    let svg_size = tree.size();
    let scale = inner / svg_size.width().max(svg_size.height());
    let tx = padding + (inner - svg_size.width() * scale) / 2.0;
    let ty = padding + (inner - svg_size.height() * scale) / 2.0;

    let transform = Transform::from_scale(scale, scale).post_translate(tx, ty);
    resvg::render(tree, transform, &mut pixmap.as_mut());

    pixmap
        .save_png(output.as_ref())
        .map_err(|e| CliError::Other(format!("failed to save PNG: {e}")))?;

    println!("  wrote {}  ({}×{})", output.as_ref().display(), size, size);
    Ok(())
}
