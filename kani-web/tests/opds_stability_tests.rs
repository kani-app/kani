#![allow(clippy::unwrap_used)]

//! `/opds` is a stable surface that stays out of the OpenAPI document, so `x-stability` cannot
//! guard it. The route set is pinned here instead: a reader's saved catalogue URL breaks if a
//! path moves, and no other test would notice a rename.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Paths promised by the OPDS surface, relative to the `/opds` nest in `main.rs`.
const STABLE_OPDS_ROUTES: &[&str] = &[
    "/",
    "/catalogue",
    "/chapters/{id}",
    "/chapters/{id}/file",
    "/chapters/{id}/page",
    "/chapters/{id}/progress",
    "/manga/{id}",
    "/opensearch",
    "/search",
];

fn mounted_routes() -> BTreeSet<String> {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/opds.rs");
    let src = std::fs::read_to_string(&file).unwrap();
    let mut out = BTreeSet::new();
    let mut cursor = 0usize;
    while let Some(rel) = src[cursor..].find(".route(") {
        let open = cursor + rel + ".route(".len();
        cursor = open;
        let rest = src[open..].trim_start();
        let Some(after_quote) = rest.strip_prefix('"') else {
            continue;
        };
        let Some(end) = after_quote.find('"') else {
            continue;
        };
        out.insert(after_quote[..end].to_string());
    }
    out
}

#[test]
fn every_promised_opds_route_is_still_mounted() {
    let mounted = mounted_routes();
    let mut missing: Vec<&str> = STABLE_OPDS_ROUTES
        .iter()
        .filter(|r| !mounted.contains(**r))
        .copied()
        .collect();
    missing.sort_unstable();

    assert!(
        missing.is_empty(),
        "{} stable OPDS route(s) are no longer served, which breaks saved reader URLs: {missing:?}",
        missing.len()
    );
}

#[test]
fn a_new_opds_route_has_to_be_classified() {
    let mounted = mounted_routes();
    let promised: BTreeSet<&str> = STABLE_OPDS_ROUTES.iter().copied().collect();
    let mut unlisted: Vec<&String> = mounted
        .iter()
        .filter(|r| !promised.contains(r.as_str()))
        .collect();
    unlisted.sort();

    assert!(
        unlisted.is_empty(),
        "{} OPDS route(s) are served but not listed in STABLE_OPDS_ROUTES — add them if they are \
         part of the promise, or document why they are not: {unlisted:?}",
        unlisted.len()
    );
}

#[test]
fn the_scan_actually_reads_the_router() {
    let mounted = mounted_routes();
    assert!(
        mounted.len() >= STABLE_OPDS_ROUTES.len(),
        "the scan found only {} routes in opds.rs, so it is not parsing the module",
        mounted.len()
    );
}
