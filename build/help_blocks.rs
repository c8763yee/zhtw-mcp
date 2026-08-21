// Extraction of the `<!-- cli:<name> -->` … `<!-- cli:end -->` help blocks
// from docs/cli.md.  Included by build.rs, which embeds the blocks into the
// binary, and by tests/cli-help.rs, which exercises the extraction and checks
// the binary's output against the docs.  One definition, two includers, so
// the tests run the same code the build ran.  Pure string processing: no
// filesystem, environment, or network access.

/// Extract every marked help block as a `(name, text)` pair, in document
/// order.
///
/// A block may wrap its text in a Markdown code fence so the docs render it
/// verbatim; the fence lines are stripped.  The text is exactly what `--help`
/// prints, with a trailing newline.
pub fn extract_cli_blocks(md: &str) -> Result<Vec<(String, String)>, String> {
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut open: Option<(String, Vec<&str>)> = None;

    for line in md.lines() {
        let Some(name) = marker_name(line) else {
            if let Some((_, lines)) = open.as_mut() {
                lines.push(line);
            }
            continue;
        };
        open = match (name == "end", open) {
            (false, None) => {
                if blocks.iter().any(|(n, _)| *n == name) {
                    return Err(format!("duplicate cli:{name} block"));
                }
                Some((name, Vec::new()))
            }
            (true, Some((opened, lines))) => {
                let text = block_text(&opened, lines)?;
                blocks.push((opened, text));
                None
            }
            (false, Some((opened, _))) => {
                return Err(format!("cli:{opened} block not closed before cli:{name}"));
            }
            (true, None) => return Err("cli:end without an open block".to_owned()),
        };
    }
    match open {
        Some((opened, _)) => Err(format!("cli:{opened} block never closed")),
        None => Ok(blocks),
    }
}

/// `Some(name)` when the line is a `<!-- cli:<name> -->` marker.
fn marker_name(line: &str) -> Option<String> {
    let inner = line
        .trim()
        .strip_prefix("<!-- cli:")?
        .strip_suffix("-->")?
        .trim();
    Some(inner.to_owned())
}

/// Turn a block's lines into help text: drop surrounding blank lines, strip
/// the optional code fence, and end with the newline `print!` relies on.
fn block_text(name: &str, mut lines: Vec<&str>) -> Result<String, String> {
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.first().is_some_and(|l| l.starts_with("```")) {
        if lines.len() < 2 || !lines.last().is_some_and(|l| l.trim() == "```") {
            return Err(format!("cli:{name} block's code fence never closes"));
        }
        lines.remove(0);
        lines.pop();
    }
    if lines.is_empty() {
        return Err(format!("cli:{name} block is empty"));
    }
    Ok(lines.join("\n") + "\n")
}
