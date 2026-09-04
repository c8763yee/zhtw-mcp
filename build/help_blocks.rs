// Extraction of the help blocks that the cli:name and cli:end markers wrap in
// docs/cli.md. Included by build.rs, which embeds the blocks into the binary,
// and by tests/cli-help.rs, which exercises the extraction and checks the
// binary's output against the docs. One definition, two includers, so the tests
// run the same code the build ran. Pure string processing: no filesystem,
// environment, or network access.

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
///
/// The fence is matched as a structure rather than by position.  Stripping a
/// leading run of backticks and then a trailing one lets an opening fence be
/// closed by whatever backticks happen to come last, so a block with prose
/// above its fence, a sample fence inside it, or two fenced sections in it all
/// built clean and shipped literal backticks to the terminal.  The guarantee
/// this file exists for is that the docs and the help text are the same text,
/// and only the whole fence pair can carry it.
fn block_text(name: &str, lines: &[&str]) -> Result<String, String> {
    let mut body = trim_blank_lines(lines);
    let fenced = body.first().and_then(|line| fence_ticks(line));

    if let Some(ticks) = fenced {
        let (last, inner) = body[1..]
            .split_last()
            .ok_or_else(|| format!("cli:{name} block's code fence never closes"))?;
        if !closes_fence(last, ticks) {
            // A closer in the middle means the fence did close and something
            // follows it, which is a different line to go and look at.
            return Err(if inner.iter().any(|line| closes_fence(line, ticks)) {
                format!("cli:{name} block has content after its code fence")
            } else {
                format!("cli:{name} block's code fence never closes")
            });
        }
        body = trim_blank_lines(inner);
    }

    if body.iter().any(|line| fence_ticks(line).is_some()) {
        return Err(match fenced {
            Some(_) => format!("cli:{name} block has a second code fence inside it"),
            None => format!("cli:{name} block's code fence must wrap the whole block"),
        });
    }

    let joined = body.join("\n");
    let text = joined.trim();
    if text.is_empty() {
        return Err(format!("cli:{name} block is empty"));
    }
    Ok(format!("{text}\n"))
}

/// The backtick count of a code fence line, or `None` when the line is not one.
fn fence_ticks(line: &str) -> Option<usize> {
    let ticks = line.trim_start().chars().take_while(|c| *c == '`').count();
    (ticks >= 3).then_some(ticks)
}

/// A closing fence carries no info string and is no shorter than its opener.
fn closes_fence(line: &str, opener: usize) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= opener && trimmed.chars().all(|c| c == '`')
}

/// The slice without the blank lines at either end.
fn trim_blank_lines<'a>(lines: &'a [&'a str]) -> &'a [&'a str] {
    let start = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map_or(start, |i| i + 1);
    &lines[start..end]
}
