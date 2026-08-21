// Tests for `--help` and `-h`.
//
// The help texts are the `<!-- cli:<name> -->` blocks in docs/cli.md,
// embedded at build time by build.rs.  Tested here in three layers: the
// block parser itself (included below, so these tests run the same code the
// build ran), the binary's output against the docs blocks, and the contracts
// the parser cannot see — the setup host list matching `setup::ALL_HOSTS`,
// and help going to stdout with exit code 0.

use std::process::{Command, Output, Stdio};

mod help_blocks {
    include!("../build/help_blocks.rs");
}
use help_blocks::extract_cli_blocks;

fn binary_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("zhtw-mcp");
    path
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
    let md = "<!-- cli:a -->\n\n```text\nbody\n```\n\n<!-- cli:end -->\n";
    let blocks = extract_cli_blocks(md).unwrap();
    assert_eq!(blocks, vec![("a".to_owned(), "body\n".to_owned())]);
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

// Contracts the docs blocks cannot enforce by construction.

#[test]
fn global_help_lists_every_subcommand() {
    let stdout = help_stdout(&["--help"]);
    for command in ["lint", "convert", "setup", "pack", "tm", "cache"] {
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
    for command in ["lint", "convert", "setup", "pack", "tm", "cache"] {
        let stdout = help_stdout(&[command, "--help"]);
        assert!(
            stdout.starts_with(&format!("zhtw-mcp {command} - ")),
            "'{command} --help' should open with its own title:\n{stdout}"
        );
    }
}

#[test]
fn lint_help_needs_no_file_argument_and_wins_over_files() {
    // Without help, bare `lint` fails for lacking files; with it, help wins
    // regardless of position.
    let stdout = help_stdout(&["lint", "a.md", "-h"]);
    assert!(stdout.starts_with("zhtw-mcp lint - "));
}

#[test]
fn setup_help_lists_every_host_run_setup_accepts() {
    // The host list is static text in docs/cli.md now, so this is what keeps
    // it in step with `setup::ALL_HOSTS`.
    let stdout = help_stdout(&["setup", "--help"]);
    for host in zhtw_mcp::mcp::setup::ALL_HOSTS {
        assert!(
            stdout.contains(host.name()),
            "setup help should list host '{}':\n{stdout}",
            host.name()
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
