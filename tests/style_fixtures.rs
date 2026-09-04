// Fixture-driven suite for the AI-filler and translationese axes.
//
// Each directory under tests/fixtures/ pairs a *_bad.txt that must fire its
// axis with one or more *_good*.txt that must not. The pairing is the whole
// point: a detector that fires on everything passes the bad half and fails the
// good half, so both directions have to hold.
//
// "writing_humanizer/" covers newer-generation LLM tells whose evidence is
// structural as well as lexical (for example slogan repetition and metaphor
// chains). Keep this suite on the full AI-review path: the base profile only
// enables individual filler rules, whereas "detect_ai" also enables the
// boundary-aware structural passes.

use std::fs;
use std::path::{Path, PathBuf};

use zhtw_mcp::engine::scan::{ContentType, Scanner};
use zhtw_mcp::rules::loader::load_embedded_ruleset;
use zhtw_mcp::rules::ruleset::{Issue, IssueType, Profile};

fn fixtures_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// One scanner for the whole suite: building it parses the embedded ruleset and
/// compiles two automata, which is not worth repeating per fixture.
fn scan(text: &str) -> Vec<Issue> {
    static SCANNER: std::sync::OnceLock<Scanner> = std::sync::OnceLock::new();
    SCANNER
        .get_or_init(|| {
            let ruleset = load_embedded_ruleset().expect("embedded ruleset loads");
            Scanner::new(ruleset.spelling_rules, ruleset.case_rules)
        })
        .scan_for_content_type_with_config(text, ContentType::Plain, {
            let mut cfg = Profile::Base.config();
            cfg.ai_semantic_safety = true;
            cfg.ai_density_detection = true;
            cfg.ai_structural_patterns = true;
            cfg
        })
        .issues
}

/// Read every `*.txt` in a fixture directory, sorted for deterministic order.
fn fixture_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("fixture dir {} unreadable: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures in {}", dir.display());
    files
}

/// The text each `axes` issue matched on in one fixture.
fn hits(path: &Path, axes: &[IssueType]) -> Vec<String> {
    let text = fs::read_to_string(path).expect("fixture readable");
    scan(&text)
        .into_iter()
        .filter(|i| axes.contains(&i.rule_type))
        .map(|i| i.found)
        .collect()
}

/// Assert that no `*_good*.txt` in `dir_name` fires any of `axes`.
///
/// Returns how many good fixtures were checked so callers can require a
/// non-zero count: a directory whose good half was deleted or renamed would
/// otherwise pass this vacuously, which is the opposite of what it is for.
fn assert_good_fixtures_clean(dir_name: &str, axes: &[IssueType]) -> usize {
    let dir = fixtures_dir(dir_name);
    let mut good_seen = 0;

    for path in fixture_files(&dir) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.contains("_good") {
            continue;
        }
        good_seen += 1;
        let found = hits(&path, axes);
        assert!(
            found.is_empty(),
            "{name}: expected no {axes:?} issue, got {found:?}"
        );
    }

    good_seen
}

/// Assert that `*_bad.txt` fires `axis` and `*_good*.txt` does not.
fn check_axis(dir_name: &str, axis: IssueType) {
    let dir = fixtures_dir(dir_name);
    let mut bad_seen = 0;

    for path in fixture_files(&dir) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with("_bad.txt") {
            bad_seen += 1;
            assert!(
                !hits(&path, &[axis]).is_empty(),
                "{name}: expected at least one {axis:?} issue, got none"
            );
        } else if !name.contains("_good") {
            panic!("{name}: fixture name must end in _bad or contain _good");
        }
    }

    let good_seen = assert_good_fixtures_clean(dir_name, &[axis]);
    assert!(
        bad_seen > 0 && good_seen > 0,
        "{dir_name}: need both bad and good fixtures, got {bad_seen} bad and {good_seen} good"
    );
}

#[test]
fn humanize_fixtures_drive_the_ai_filler_axis() {
    check_axis("humanize", IssueType::AiStyle);
}

#[test]
fn translationese_fixtures_drive_the_translationese_axis() {
    check_axis("translationese", IssueType::Translationese);
}

#[test]
fn writing_humanizer_fixtures_drive_the_ai_filler_axis() {
    check_axis("writing_humanizer", IssueType::AiStyle);
}

#[test]
fn newer_llm_tell_fixtures_drive_the_ai_filler_axis() {
    let dir = fixtures_dir("writing_humanizer");

    // Assert per phrase, not merely that the file produced something. With a
    // bare "!found.is_empty()" any one rule firing satisfied the whole file, so
    // four of pattern32's five rules could rot undetected.
    for (name, phrases) in [
        (
            "pattern32_bad.txt",
            &["截至我所掌握的資訊", "沒有人告訴你的是", "大家都錯了"][..],
        ),
        (
            "pattern33_bad.txt",
            &["很可能出身於", "為人低調鮮少公開"][..],
        ),
        ("pattern35_bad.txt", &["最妙的是：", "更可怕的是："][..]),
        (
            "pattern36_bad.txt",
            &[
                "大多數人都搞錯了",
                "大部分人都搞錯了",
                "這是90%的人忽略的",
                "這是 90% 的人忽略的",
            ][..],
        ),
    ] {
        let found = hits(&dir.join(name), &[IssueType::AiStyle]);
        for phrase in phrases {
            assert!(
                found.iter().any(|hit| hit == phrase),
                "{name}: missing match for {phrase}; got {found:?}"
            );
        }
    }
}

/// The good half of the pending directory must already be clean, otherwise
/// whatever detector lands for these patterns starts from a false positive.
#[test]
fn writing_humanizer_good_fixtures_are_already_clean() {
    let good_seen = assert_good_fixtures_clean(
        "writing_humanizer",
        &[IssueType::AiStyle, IssueType::Translationese],
    );
    assert!(good_seen > 0, "writing_humanizer has no good fixtures left");
}
