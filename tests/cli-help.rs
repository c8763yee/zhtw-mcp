// Tests for --help and -h.
//
// The help texts are the cli:<name> blocks in docs/cli.md, embedded at
// build time by build.rs.  Tested here in three layers: the block parser
// itself (included below, so these tests run the same code the build ran),
// the binary's output against the docs blocks, and the contracts the parser
// cannot see: the setup host list matching setup::ALL_HOSTS, and help going
// to stdout with exit code 0.

use std::process::{Command, Output, Stdio};

#[path = "../build/help_blocks.rs"]
mod help_blocks;
use help_blocks::extract_cli_blocks;

fn binary_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_zhtw-mcp"))
}

fn run(args: &[&str]) -> Output {
    Command::new(binary_path())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("RUST_LOG")
        .output()
        .unwrap()
}

/// Run a help invocation and return its stdout, asserting the exit-0 /
/// stdout-only contract every help form must satisfy.
fn help_stdout(args: &[&str]) -> String {
    let output = run(args);
    assert!(output.status.success(), "{args:?} should exit 0");
    assert!(
        output.stderr.is_empty(),
        "{args:?} should not write stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Usage:"),
        "{args:?} should print a usage section:\n{stdout}"
    );
    stdout
}

// The parser.

#[test]
fn extracts_blocks_in_document_order() {
    let md = "intro\n<!-- cli:a -->\nfirst\n<!-- cli:end -->\nprose\n\n<!-- cli:b -->\nsecond\nline\n<!-- cli:end -->\n";
    let blocks = extract_cli_blocks(md).unwrap();
    assert_eq!(
        blocks,
        vec![
            ("a".to_owned(), "first\n".to_owned()),
            ("b".to_owned(), "second\nline\n".to_owned()),
        ]
    );
}

#[test]
fn strips_the_code_fence_and_surrounding_blank_lines() {
    // Blank lines on both sides of the fence: the ones outside it and the
    // ones just inside it are equally not part of the help text.
    let md = "<!-- cli:a -->\n\n```text\n\nbody\n\n```\n\n<!-- cli:end -->\n";
    let blocks = extract_cli_blocks(md).unwrap();
    assert_eq!(blocks, vec![("a".to_owned(), "body\n".to_owned())]);
}

#[test]
fn fenced_and_unfenced_blocks_end_in_one_newline() {
    let fenced = "<!-- cli:a -->\n```text\nbody\n\n```\n<!-- cli:end -->\n";
    let bare = "<!-- cli:a -->\nbody\n<!-- cli:end -->\n";
    assert_eq!(
        extract_cli_blocks(fenced).unwrap(),
        extract_cli_blocks(bare).unwrap()
    );
}

#[test]
fn malformed_blocks_are_errors() {
    for (md, expected) in [
        ("<!-- cli:a -->\nbody\n", "cli:a block never closed"),
        (
            "<!-- cli:a -->\n<!-- cli:b -->\n<!-- cli:end -->\n",
            "cli:a block not closed before cli:b",
        ),
        ("<!-- cli:end -->\n", "cli:end without an open block"),
        (
            "<!-- cli:a -->\nx\n<!-- cli:end -->\n<!-- cli:a -->\ny\n<!-- cli:end -->\n",
            "duplicate cli:a block",
        ),
        (
            "<!-- cli:a -->\n```text\nbody\n<!-- cli:end -->\n",
            "cli:a block's code fence never closes",
        ),
        (
            "<!-- cli:a -->\n\n<!-- cli:end -->\n",
            "cli:a block is empty",
        ),
    ] {
        assert_eq!(
            extract_cli_blocks(md).unwrap_err(),
            expected,
            "for input:\n{md}"
        );
    }
}

// The binary against the docs.

#[test]
fn help_output_matches_the_docs_blocks() {
    let md = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/cli.md")).unwrap();
    let blocks = extract_cli_blocks(&md).unwrap();
    assert!(!blocks.is_empty(), "docs/cli.md should carry cli blocks");
    for (name, text) in &blocks {
        let args: Vec<&str> = match name.as_str() {
            "global" => vec!["--help"],
            _ => vec![name, "--help"],
        };
        assert_eq!(
            &help_stdout(&args),
            text,
            "{args:?} should print the cli:{name} block of docs/cli.md"
        );
    }
}

/// Every subcommand, taken from the docs blocks rather than retyped.
///
/// build.rs already rejects a block without a topic and a topic without a
/// block, so this is the one list of subcommands a test can read without
/// becoming another copy to keep in step.
fn subcommands() -> Vec<String> {
    let md = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/cli.md")).unwrap();
    let names: Vec<String> = extract_cli_blocks(&md)
        .unwrap()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name != "global")
        .collect();
    assert!(!names.is_empty(), "docs/cli.md should carry cli blocks");
    names
}

// Contracts the docs blocks cannot enforce by construction.

#[test]
fn global_help_lists_every_subcommand() {
    let stdout = help_stdout(&["--help"]);
    for command in subcommands() {
        assert!(
            stdout.contains(&format!("\n  {command} ")),
            "global help should list '{command}':\n{stdout}"
        );
    }
    assert!(
        stdout.contains("zhtw-mcp <command> --help"),
        "global help should point at subcommand help:\n{stdout}"
    );
}

#[test]
fn short_flag_matches_global_help() {
    assert_eq!(
        help_stdout(&["--help"]),
        help_stdout(&["-h"]),
        "-h should match --help"
    );
}

#[test]
fn each_subcommand_prints_its_own_message() {
    // A subcommand with no row in SUBCOMMAND_TOPICS prints the global message
    // instead of its own, and this is what says so.
    for command in subcommands() {
        let stdout = help_stdout(&[&command, "--help"]);
        assert!(
            stdout.starts_with(&format!("zhtw-mcp {command} - ")),
            "'{command} --help' should open with its own title:\n{stdout}"
        );
    }
}

#[test]
fn lint_help_needs_no_file_argument_and_wins_over_files() {
    // Without help, bare lint fails for lacking files; with it, help wins
    // regardless of position.
    let stdout = help_stdout(&["lint", "a.md", "-h"]);
    assert!(stdout.starts_with("zhtw-mcp lint - "));
}

/// The names in the Hosts section of setup help, sorted.
///
/// The section runs from the Hosts heading to the next blank line.  A name is
/// followed either by a comma or, where the entry carries a description, by
/// the run of spaces that separates the two columns.
fn listed_hosts(help: &str) -> Vec<&str> {
    help.lines()
        .skip_while(|line| line.trim() != "Hosts:")
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .flat_map(|line| line.trim().split("  ").next().unwrap_or("").split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[test]
fn setup_help_lists_every_host_run_setup_accepts() {
    // Both directions, because each failure hurts a different person.  A host
    // the help omits is a host nobody finds; a host the help names and setup
    // rejects wastes the time of somebody who followed the instructions.
    //
    // ALL_HOSTS carries the canonical spellings, and translation-guide is
    // handled ahead of them in main.rs, so the expected set is the two
    // together.  Aliases such as claude-code and translation_guide stay
    // unlisted on purpose: from_name accepts several spellings per host and
    // the help names one.
    let stdout = help_stdout(&["setup", "--help"]);
    let listed = listed_hosts(&stdout);

    let mut expected: Vec<&str> = zhtw_mcp::mcp::setup::ALL_HOSTS
        .iter()
        .map(|host| host.name())
        .chain(std::iter::once("translation-guide"))
        .collect();
    expected.sort_unstable();
    assert_eq!(listed, expected, "setup help lists the wrong hosts");

    // Run each one rather than trusting from_name: the guide is dispatched
    // before from_name is ever reached, so only the binary answers for it.
    for host in listed {
        let output = run(&["setup", host]);
        assert!(
            output.status.success(),
            "setup help lists '{host}', which setup rejects: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn help_subcommand_does_not_exist() {
    let output = run(&["help"]);
    assert!(!output.status.success(), "'help' should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown argument: help"),
        "'help' should be an unknown argument: {stderr}"
    );
}
