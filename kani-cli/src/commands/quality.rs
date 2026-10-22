use crate::error::CliError;
use std::path::Path;

/// Prints the quality score and per-page dimensions for a CBZ — the numbers
/// `upgrade_min_res_gain` is tuned against.
pub fn score(path: &Path) -> Result<(), CliError> {
    let m = kani_core::manifest::manifest_for_cbz(path)
        .map_err(|e| CliError::Other(format!("cannot read {}: {e}", path.display())))?;
    let s = kani_core::quality::score_from_manifest(&m);

    println!("{}", path.display());
    println!("  pages            {}", s.page_count);
    println!("  median long edge {} px", s.median_long_edge_px);
    println!("  bytes/megapixel  {:.0}", s.bytes_per_megapixel);
    println!("  total bytes      {}", m.total_bytes);
    println!();
    for p in &m.pages {
        let dims = match (p.width, p.height) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => "?".to_string(),
        };
        println!("  {:<24} {:>10} {:>10} B", p.name, dims, p.bytes);
    }
    Ok(())
}

/// Per-page perceptual-hash distance between two CBZs. Zero means visually
/// identical pages; a large distance means the pages differ, not merely the
/// encoding.
pub(crate) fn phash_compare(a: &Path, b: &Path) -> Result<(), CliError> {
    let ma = kani_core::manifest::manifest_for_cbz(a)
        .map_err(|e| CliError::Other(format!("cannot read {}: {e}", a.display())))?;
    let mb = kani_core::manifest::manifest_for_cbz(b)
        .map_err(|e| CliError::Other(format!("cannot read {}: {e}", b.display())))?;

    println!("{}  vs  {}", a.display(), b.display());
    println!("  pages: {} vs {}", ma.page_count, mb.page_count);
    println!();

    let n = ma.pages.len().min(mb.pages.len());

    let mut total = 0u32;
    for i in 0..n {
        let d = kani_core::quality::phash_distance(
            ma.pages[i].perceptual_hash,
            mb.pages[i].perceptual_hash,
        );
        total += d;
        println!("  {:<3} {:<24} distance {d}", i + 1, ma.pages[i].name);
    }
    if n > 0 {
        println!();
        println!("  mean distance {:.2}", f64::from(total) / n as f64);
    }
    if ma.pages.len() != mb.pages.len() {
        println!("  note: page counts differ; compared the first {n} page(s) only");
    }
    Ok(())
}

/// Reports what a header probe can learn about a local image, mirroring exactly
/// what upgrade detection reads from a remote page's first few kilobytes.
pub fn probe(path: &Path) -> Result<(), CliError> {
    let bytes = std::fs::read(path)
        .map_err(|e| CliError::Other(format!("cannot read {}: {e}", path.display())))?;
    let prefix = &bytes[..bytes.len().min(kani_core::probe::PROBE_PREFIX_BYTES)];
    let p = kani_core::probe::probe_header(prefix, Some(bytes.len() as u64));

    println!("{}", path.display());
    println!("  read {} of {} bytes", prefix.len(), bytes.len());
    match (p.width, p.height) {
        (Some(w), Some(h)) => println!("  dimensions      {w}x{h}"),
        _ => println!("  dimensions      unreadable"),
    }
    println!("  bytes           {}", p.bytes.unwrap_or(0));
    println!(
        "  colour          {}",
        match p.colour {
            Some(true) => "yes",
            Some(false) => "greyscale",
            None => "unknown (three-component JPEG)",
        }
    );
    match p.jpeg_quality {
        Some(q) => println!("  jpeg quality    ~{q}"),
        None => println!("  jpeg quality    n/a"),
    }
    Ok(())
}
