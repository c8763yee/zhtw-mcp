// Extraction of the help blocks that the cli:name and cli:end markers wrap
// in docs/cli.md.  Included by build.rs, which embeds the blocks into the
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
                let text = block_text(&opened, &lines)?;
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

/// Turn a block's lines into help text: strip the optional code fence, drop
/// the blank lines around it and inside it, and end with the newline
/// `print!` relies on.
fn block_text(name: &str, lines: &[&str]) -> Result<String, String> {
    let joined = lines.join("\n");
    let mut body = joined.trim();
    if let Some(rest) = body.strip_prefix("```") {
        body = rest
            .split_once('\n')
            .and_then(|(_, inner)| inner.trim_end().strip_suffix("```"))
            .ok_or_else(|| format!("cli:{name} block's code fence never closes"))?
            .trim();
    }
    if body.is_empty() {
        return Err(format!("cli:{name} block is empty"));
    }
    Ok(format!("{body}\n"))
}
