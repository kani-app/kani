#![allow(clippy::unwrap_used)]

//! The frontend hides and shows whole surfaces on `hasPermission('resource:action')`.
//! Nothing checked that those strings name permissions the server actually
//! grants, so a typo or a rename hides a feature silently and forever — there is
//! no error, the entry simply never appears.

use kani_app::permissions::Permission;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn frontend_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../static/js");
    let mut out = Vec::new();
    collect(&root, &mut out);
    assert!(
        out.len() > 20,
        "expected the frontend sources under {}, found {}",
        root.display(),
        out.len()
    );
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "dist" || n == "vendor")
            {
                continue;
            }
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "js") {
            out.push(path);
        }
    }
}

/// Every `'resource:action'` literal handed to a permission check in the UI.
fn permissions_used_by_the_frontend() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for path in frontend_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        for marker in ["hasPermission(", "perm: "] {
            let mut cursor = 0usize;
            while let Some(rel) = src[cursor..].find(marker) {
                let at = cursor + rel + marker.len();
                cursor = at;
                let rest = src[at..].trim_start();
                let Some(inner) = rest.strip_prefix('\'').or_else(|| rest.strip_prefix('"')) else {
                    continue;
                };
                let Some(end) = inner.find(['\'', '"']) else {
                    continue;
                };
                let literal = &inner[..end];
                if literal.contains(':') && !literal.contains(' ') {
                    found.insert(literal.to_string());
                }
            }
        }
    }
    found
}

#[test]
fn every_permission_the_ui_checks_exists_on_the_server() {
    let used = permissions_used_by_the_frontend();
    assert!(
        used.len() >= 10,
        "the scan found only {} permission literals, so it is not looking in the right place",
        used.len()
    );

    let unknown: Vec<&String> = used
        .iter()
        .filter(|p| p.parse::<Permission>().is_err())
        .collect();

    assert!(
        unknown.is_empty(),
        "the UI gates features on permissions the server does not define, so those \
         features are invisible to everyone: {unknown:?}"
    );
}

#[test]
fn the_scan_would_notice_a_typo() {
    assert!("library:view".parse::<Permission>().is_ok());
    assert!(
        "libary:view".parse::<Permission>().is_err(),
        "typo in the resource"
    );
    assert!(
        "library:veiw".parse::<Permission>().is_err(),
        "typo in the action"
    );
    assert!("library".parse::<Permission>().is_err(), "missing action");
}
