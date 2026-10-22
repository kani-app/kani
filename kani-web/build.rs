use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../migrations");

    // Prefer an injected GIT_SHA: Docker builds exclude .git/ (and ship no git
    // binary), so shelling out there would silently yield an empty SHA and make
    // support bundles from the primary deployment mode unattributable.
    println!("cargo:rerun-if-env-changed=GIT_SHA");
    let git_sha = std::env::var("GIT_SHA")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        });
    println!("cargo:rustc-env=GIT_SHA={git_sha}");

    let is_release = std::env::var("PROFILE").unwrap_or_default() == "release";

    copy_changelog();
    build_css(is_release);

    if is_release {
        build_js();
        stage_assets_for_embedding();
    }
}

/// Copies the built frontend into `OUT_DIR/assets`, which is what the binary
/// embeds on a release build.
///
/// Staged rather than embedded straight from `../static`: the assets above are
/// *generated*, and cargo does not tie a recompile of this crate to files a
/// build script writes outside `OUT_DIR`. Embedding the source tree directly
/// therefore risks shipping a binary whose frontend is a build behind — which
/// fails silently, because everything works and the UI is merely old.
///
/// Only what the server actually serves is copied. `static/js` in particular
/// contributes `dist/` alone: the unbundled modules are inputs to esbuild, and
/// embedding them would put the whole frontend source inside the binary.
fn stage_assets_for_embedding() {
    let out = std::path::PathBuf::from(
        std::env::var("OUT_DIR").expect("OUT_DIR is always set for a build script"),
    )
    .join("assets");
    let _ = std::fs::remove_dir_all(&out);

    let pairs: [(&str, &str); 6] = [
        ("../static/js/dist", "js/dist"),
        ("../static/css", "css"),
        ("../static/fonts", "fonts"),
        ("../static/icons", "icons"),
        ("../static/locales", "locales"),
        ("../static/img", "img"),
    ];
    for (src, dest) in pairs {
        let src = Path::new(src);
        if src.is_dir() {
            copy_tree(src, &out.join(dest));
        }
    }

    // chart.umd.min.js is not an esbuild input: stats.js loads it as a plain
    // <script> for its UMD global, so it needs embedding on its own.
    let chart_src = Path::new("../static/js/vendor/chart.umd.min.js");
    if chart_src.is_file() {
        let dest = out.join("js/vendor/chart.umd.min.js");
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::copy(chart_src, &dest) {
            println!("cargo:warning=cannot stage {}: {e}", chart_src.display());
        }
    }

    for name in [
        "index.prod.html",
        "manifest.webmanifest",
        "sw.js",
        "changelog.md",
    ] {
        let src = Path::new("../static").join(name);
        if src.is_file() {
            let dest = out.join(name);
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::copy(&src, &dest) {
                println!("cargo:warning=cannot stage {}: {e}", src.display());
            }
        }
    }
}

fn copy_tree(src: &Path, dest: &Path) {
    if std::fs::create_dir_all(dest).is_err() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let target = dest.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else if let Err(e) = std::fs::copy(&path, &target) {
            println!("cargo:warning=cannot stage {}: {e}", path.display());
        }
    }
}

fn copy_changelog() {
    println!("cargo:rerun-if-changed=../CHANGELOG.md");
    if let Err(e) = std::fs::copy("../CHANGELOG.md", "../static/changelog.md") {
        println!("cargo:warning=failed to copy CHANGELOG.md into static/: {e}");
    }
}

fn build_css(minify: bool) {
    println!("cargo:rerun-if-changed=../static/css/app.css");
    println!("cargo:rerun-if-changed=../static/js");

    let esbuild_path = if cfg!(windows) {
        "../tools/tailwindcss.exe"
    } else {
        "../tools/tailwindcss"
    };
    println!("cargo:rerun-if-changed={}", esbuild_path);

    let binary = Path::new(esbuild_path).to_path_buf();

    if !binary.exists() {
        println!(
            "cargo:warning=Tailwind CLI not found at {}; skipping CSS build. \
             Run `kani-cli setup --tailwind` to download it.",
            binary.display()
        );
        return;
    }

    let mut cmd = std::process::Command::new(&binary);
    cmd.args([
        "-i",
        "../static/css/app.css",
        "-o",
        "../static/css/main.css",
    ]);
    if minify {
        cmd.arg("--minify");
    }

    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", binary.display()));

    if !status.success() {
        panic!("tailwindcss CSS build failed (exit {})", status);
    }
}

fn build_js() {
    println!("cargo:rerun-if-changed=../static/js");

    let esbuild_path = if cfg!(windows) {
        "../tools/esbuild.exe"
    } else {
        "../tools/esbuild"
    };
    println!("cargo:rerun-if-changed={}", esbuild_path);

    let binary = Path::new(esbuild_path).to_path_buf();

    if !binary.exists() {
        panic!(
            "esbuild not found at {}. \n\
             Run `kani-cli setup --esbuild` to download it.",
            binary.display()
        );
    }

    // esbuild names split chunks by content hash, so a rebuild adds files rather
    // than replacing them, and `stage_assets_for_embedding` copies whatever it
    // finds. Without this the binary carries every past build's chunks.
    let dist = Path::new("../static/js/dist");
    let _ = std::fs::remove_dir_all(dist);
    std::fs::create_dir_all(dist).expect("failed to create static/js/dist");

    let status = std::process::Command::new(&binary)
        .args([
            "../static/js/app.js",
            "--bundle",
            "--splitting",
            "--format=esm",
            "--minify",
            "--platform=browser",
            // Dev-only routes sit behind this, so the branch and anything it
            // lazily imports are eliminated rather than shipped as an unreachable chunk.
            "--define:__KANI_DEV__=false",
            "--outdir=../static/js/dist",
            "--alias:preact=../static/js/vendor/preact.module.js",
            "--alias:preact/hooks=../static/js/vendor/preact-hooks.module.js",
            "--alias:htm=../static/js/vendor/htm.module.js",
            "--alias:@preact/signals-core=../static/js/vendor/signals-core.module.js",
            "--alias:@preact/signals=../static/js/vendor/signals.module.js",
            "--alias:preact/compat=../static/js/vendor/compat.module.js",
        ])
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", binary.display()));

    if !status.success() {
        panic!("esbuild JS bundle failed (exit {})", status);
    }
}
