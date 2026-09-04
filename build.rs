// Build script: pre-serialize assets/ruleset.json to postcard binary format. At
// runtime, postcard::from_bytes is ~10x faster than serde_json::from_str.

use std::path::Path;

// The wire-format types, pulled straight from the runtime source rather than
// mirrored here. Postcard is not self-describing: field order and field count
// are the encoding, so a hand-kept second copy corrupts every rule the moment
// it drifts, silently and at runtime. One definition cannot drift.
//
// Everything the include!d file needs is in scope below and nothing else from
// the crate is referenced, which is a constraint on that file rather than on
// this one. See src/rules/schema.rs.
#[allow(dead_code)]
mod schema {
    include!("src/rules/schema.rs");
}
use schema::Ruleset;

fn main() {
    let ruleset_path = Path::new("assets/ruleset.json");
    println!("cargo:rerun-if-changed={}", ruleset_path.display());

    let json = std::fs::read_to_string(ruleset_path).expect("read assets/ruleset.json");
    let ruleset: Ruleset = serde_json::from_str(&json).expect("parse ruleset.json");

    let bytes = postcard::to_allocvec(&ruleset).expect("postcard serialize");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let out_path = Path::new(&out_dir).join("ruleset.postcard");
    std::fs::write(&out_path, &bytes).expect("write ruleset.postcard");

    emit_engine_fingerprint();
    install_git_hooks();
}

/// Install the git hooks into a checkout.
///
/// A hook nobody installed is a hook nobody runs, and the first person to find
/// that out is whoever reads a commit message the rules would have caught. A
/// build is the one step every contributor takes, so it is where installation
/// belongs; "make hooks" stays for anyone who wants it on its own.
///
/// Never fails the build. A checkout can be read-only, a hooks directory can
/// belong to somebody else's tooling, and a Windows build has no sh. None of
/// that is a reason to refuse to compile, so a failure is a warning and the
/// build goes on.
fn install_git_hooks() {
    let Some(root) = std::env::var_os("CARGO_MANIFEST_DIR").map(std::path::PathBuf::from) else {
        return;
    };

    // A linked worktree has a .git file rather than a directory, and both mean
    // the same thing here: this is a checkout, so it has hooks worth
    // installing.
    if !root.join(".git").exists() {
        return;
    }

    // So does a checkout cargo made for a git dependency, under CARGO_HOME or
    // its default of $HOME/.cargo. Nothing there is anybody's working tree: the
    // hooks would go into a directory cargo shares between every consumer of
    // this crate and never runs, and the two lines the installer prints for a
    // person would reach somebody building something else entirely.
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".cargo"))
        });
    if cargo_home.is_some_and(|cargo_home| root.starts_with(cargo_home)) {
        return;
    }

    // Asked of git rather than assumed to be .git/hooks: a linked worktree
    // keeps its hooks with the common directory, and core.hooksPath moves them
    // anywhere. Watching the wrong directory is a watch that never fires.
    //
    // The watch is what brings a deleted hook back. It costs one extra rerun of
    // this script after the first build of a fresh clone, because installing
    // the hooks is itself a change to the directory being watched. This build
    // script already reruns on any change under src, so that is the whole of
    // the price here; "make hooks" is the same install without the watch.
    if let Some(hooks) = hooks_dir(&root) {
        if hooks.is_dir() {
            println!("cargo:rerun-if-changed={}", hooks.display());
        }
    }
    println!("cargo:rerun-if-changed=scripts/install-git-hooks.sh");

    let installer = root.join("scripts/install-git-hooks.sh");
    if !installer.is_file() {
        return;
    }

    // Git for Windows carries a shell, but nothing guarantees it is on PATH.
    // Silent rather than a warning: the release workflow builds a Windows
    // target on every push to main, and a warning nobody can act on teaches
    // people to skim them.
    if cfg!(windows) {
        return;
    }

    match std::process::Command::new("sh")
        .arg(&installer)
        .current_dir(&root)
        .output()
    {
        Ok(output) if output.status.success() => {
            // Cargo swallows a build script's stdout, which is right for the
            // four lines naming a hook it installed and wrong for the two the
            // installer prints when it needs a person: a hook it refused to
            // overwrite, and a core.hooksPath that sends hooks somewhere else.
            // Those two reach the terminal or nothing does.
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let line = line.trim();
                if line.starts_with("KEEP") || line.starts_with("NOTE") {
                    println!("cargo::warning=git hooks: {line}");
                }
            }
        }
        Ok(output) => warn_hooks(&String::from_utf8_lossy(&output.stderr)),
        Err(error) => warn_hooks(&error.to_string()),
    }
}

/// Where git keeps this checkout's hooks, which is .git/hooks only in the
/// simple case.
fn hooks_dir(root: &Path) -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-path", "hooks"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    Some(std::path::PathBuf::from(path.trim()))
}

fn warn_hooks(reason: &str) {
    let reason = reason.trim().replace('\n', "; ");
    let reason = if reason.is_empty() {
        "scripts/install-git-hooks.sh failed".to_string()
    } else {
        reason
    };
    println!("cargo::warning=could not install the git hooks ({reason}); run make hooks");
}

/// Hash the scanner sources into `ZHTW_ENGINE_FINGERPRINT`.
///
/// The scan cache keys on this. A crate version is not enough: it only moves at
/// a release bump, so a source build, a git checkout, or a detector fix within
/// one release would keep serving the previous scanner's results for every
/// unchanged file. Hashing the sources means the key moves exactly when the
/// code that produced the cached answer moves. Cargo.lock is hashed alongside
/// the sources, since the dependencies do as much of the scanning as we do.
///
/// `DefaultHasher::new()` has fixed keys and files are hashed in sorted path
/// order, so the value is stable for a given toolchain. std does not promise
/// the algorithm across releases, so a rustc upgrade can change it. That is the
/// right failure direction for a cache key: the worst case is a cold cache
/// after a toolchain change, never a stale hit. Nothing else depends on this
/// value, so it does not affect reproducible builds of the binary itself.
fn emit_engine_fingerprint() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    println!("cargo:rerun-if-changed=src");

    // Dependency versions are part of the scanner too: a cargo update can
    // change how pulldown-cmark, aho-corasick or the normalizer behave with an
    // untouched src/ and an unchanged crate version.
    println!("cargo:rerun-if-changed=Cargo.lock");

    let mut files = Vec::new();
    collect_rs_files(Path::new("src"), &mut files);
    files.sort();

    let mut hasher = DefaultHasher::new();
    std::fs::read("Cargo.lock")
        .unwrap_or_default()
        .hash(&mut hasher);
    for path in &files {
        // The path matters as well as the bytes: moving code between modules
        // can change behavior through cfg gating alone.
        path.to_string_lossy().hash(&mut hasher);

        // A file that vanished mid-build still has to produce a stable value
        // rather than abort the build.
        std::fs::read(path).unwrap_or_default().hash(&mut hasher);
    }
    println!(
        "cargo:rustc-env=ZHTW_ENGINE_FINGERPRINT={:016x}",
        hasher.finish()
    );
}

fn collect_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
