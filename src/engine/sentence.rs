// Sentence and paragraph boundary index.
//
// Provides reusable boundary detection for grammar checks, structural AI
// pattern detection, and translationese scoring. Computed once per scan, shared
// across consumers.
//
// Chinese sentence boundaries: 。？！；and blank-line paragraph breaks. Mixed
// CJK/Latin: also split on .?! followed by whitespace + uppercase. Abbreviation
// deny-list prevents false splits (Mr., P.S., etc.).

use crate::engine::excluded::{is_excluded, ByteRange};

/// A sentence span identified by byte offsets into the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SentenceBound {
    pub byte_start: usize,
    pub byte_end: usize,
}

/// A paragraph span identified by byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParagraphBound {
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Pre-computed sentence and paragraph boundary index for a document.
#[derive(Debug)]
pub struct BoundaryIndex {
    pub sentences: Vec<SentenceBound>,
    pub paragraphs: Vec<ParagraphBound>,
}

// CJK terminal punctuation that always ends a sentence.
const CJK_TERMINATORS: &[char] = &['。', '！', '？'];

// Semicolons act as sentence boundaries in Chinese text.
const CJK_SOFT_TERMINATORS: &[char] = &['；'];

// Latin terminal punctuation, which only triggers a split when followed by
// whitespace + uppercase letter (to avoid splitting on abbreviations).
const LATIN_TERMINATORS: &[char] = &['.', '?', '!'];

// Abbreviation patterns that end with '.' but are NOT sentence boundaries.
// Checked by looking at the text preceding a Latin period.
const ABBREVIATION_SUFFIXES: &[&str] = &[
    "Mr", "Mrs", "Ms", "Dr", "Prof", "Jr", "Sr", "vs", "etc", "i.e", "e.g", "P.S", "p.s", "cf",
    "al", "Vol", "No", "Fig", "Eq", "Rev",
];

impl BoundaryIndex {
    /// Build a boundary index for the given text, respecting exclusion zones.
    ///
    /// Exclusion zones (code blocks, URLs, etc.) are treated as opaque:
    /// boundaries inside them are ignored, and entering/leaving an exclusion
    /// zone acts as a sentence break.
    pub fn build(text: &str, excluded: &[ByteRange]) -> Self {
        crate::engine::index_guard::note_build(crate::engine::index_guard::DocIndex::Boundary);
        let sentences = build_sentences(text, excluded);
        let paragraphs = build_paragraphs(text);
        BoundaryIndex {
            sentences,
            paragraphs,
        }
    }

    /// Find the sentence containing byte offset `pos`.
    /// Returns None if pos is outside all sentences (e.g. inside an exclusion
    /// zone).
    pub fn sentence_at(&self, pos: usize) -> Option<&SentenceBound> {
        self.sentences
            .iter()
            .find(|s| s.byte_start <= pos && pos < s.byte_end)
    }

    /// Find the paragraph containing byte offset `pos`.
    pub fn paragraph_at(&self, pos: usize) -> Option<&ParagraphBound> {
        self.paragraphs
            .iter()
            .find(|p| p.byte_start <= pos && pos < p.byte_end)
    }

    /// Return all sentences within a given paragraph.
    ///
    /// Both lists are built in offset order, so the sentences of a paragraph
    /// are a contiguous run and two binary searches find it. The filter this
    /// replaced walked every sentence and allocated a Vec per paragraph, which
    /// is quadratic in the document: on a 200 KB file with 970 paragraphs and
    /// 3,395 sentences it cost 2.2 ms, most of the closing-phrase pass.
    pub fn sentence_slice(&self, para: &ParagraphBound) -> &[SentenceBound] {
        let start = self
            .sentences
            .partition_point(|s| s.byte_start < para.byte_start);
        let end = self
            .sentences
            .partition_point(|s| s.byte_end <= para.byte_end);
        self.sentences.get(start..end).unwrap_or(&[])
    }

    /// Extract the text slice for a sentence bound.
    pub fn sentence_text<'a>(&self, text: &'a str, s: &SentenceBound) -> &'a str {
        &text[s.byte_start..s.byte_end]
    }

    /// Extract the text slice for a paragraph bound.
    pub fn paragraph_text<'a>(&self, text: &'a str, p: &ParagraphBound) -> &'a str {
        &text[p.byte_start..p.byte_end]
    }
}

/// Build sentence boundaries from text.
fn build_sentences(text: &str, excluded: &[ByteRange]) -> Vec<SentenceBound> {
    let mut sentences = Vec::new();
    let mut sent_start: usize = 0;
    let mut in_excluded = false;
    let mut last_was_content = false;

    let bytes = text.as_bytes();
    let mut byte_offset = 0;

    for ch in text.chars() {
        let ch_len = ch.len_utf8();
        let ch_end = byte_offset + ch_len;

        // Handle exclusion zone transitions.
        let currently_excluded = is_excluded(byte_offset, ch_end, excluded);
        if currently_excluded && !in_excluded {
            // Entering exclusion: flush current sentence if non-empty.
            if last_was_content && byte_offset > sent_start {
                push_sentence(&mut sentences, text, sent_start, byte_offset);
            }
            in_excluded = true;
            last_was_content = false;
        } else if !currently_excluded && in_excluded {
            // Leaving exclusion: start new sentence.
            sent_start = byte_offset;
            in_excluded = false;
        }

        if currently_excluded {
            byte_offset = ch_end;
            continue;
        }

        // The same blank-line rule the paragraph splitters use: a newline, an
        // optional "\r", another newline. Recognising only the two pure forms
        // left a mixed "\n\r\n" document with paragraph bounds that no sentence
        // bound matched, so "sentence_slice" returned nothing for either
        // paragraph and every paragraph-scoped detector skipped the content.
        let paragraph_break_len = match ch {
            // Locate the "\n" of the first terminator, whichever form it takes.
            '\r' if bytes.get(ch_end) == Some(&b'\n') => blank_line_end(bytes, ch_end),
            '\n' => blank_line_end(bytes, byte_offset),
            _ => None,
        }
        .map(|end| end - byte_offset);

        // Paragraph break, in any combination of terminators.
        if let Some(break_len) = paragraph_break_len {
            if last_was_content && byte_offset > sent_start {
                push_sentence(&mut sentences, text, sent_start, byte_offset);
            }
            last_was_content = false;
            sent_start = byte_offset + break_len;
            byte_offset = ch_end;
            continue;
        }

        // CJK hard terminators: always split.
        if CJK_TERMINATORS.contains(&ch) || CJK_SOFT_TERMINATORS.contains(&ch) {
            // Include the terminator in the sentence.
            push_sentence(&mut sentences, text, sent_start, ch_end);
            sent_start = ch_end;
            last_was_content = false;
            byte_offset = ch_end;
            continue;
        }

        // Latin terminators: split only if followed by whitespace + uppercase.
        if LATIN_TERMINATORS.contains(&ch) && is_latin_sentence_end(text, byte_offset, ch_end) {
            push_sentence(&mut sentences, text, sent_start, ch_end);
            sent_start = ch_end;
            last_was_content = false;
            byte_offset = ch_end;
            continue;
        }

        if !ch.is_whitespace() {
            last_was_content = true;
        }

        byte_offset = ch_end;
    }

    // Flush trailing sentence.
    if sent_start < text.len() && last_was_content {
        push_sentence(&mut sentences, text, sent_start, text.len());
    }

    sentences
}

/// Check if a Latin period/question/exclamation at `dot_start..dot_end` is a
/// real sentence boundary (followed by whitespace + uppercase letter) and not
/// an abbreviation.
fn is_latin_sentence_end(text: &str, dot_start: usize, dot_end: usize) -> bool {
    let rest = &text[dot_end..];
    let mut chars = rest.chars().peekable();

    // Must be followed by at least one whitespace char; consume the full
    // whitespace run rather than the first, so a period followed by two spaces
    // and a capital still splits.
    match chars.peek() {
        Some(c) if c.is_whitespace() => {}
        _ => return false,
    }
    while let Some(c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }

    // Then an uppercase letter (or CJK, which counts as a new sentence).
    match chars.next() {
        Some(c) if c.is_uppercase() || is_cjk(c) => {}
        _ => return false,
    }

    // Check abbreviation deny-list: look at text before the dot. Word boundary
    // = start of text, ASCII whitespace/punct, or CJK char (no ASCII letter
    // immediately before the abbreviation).
    let before = &text[..dot_start];
    for abbr in ABBREVIATION_SUFFIXES {
        if before.ends_with(abbr) {
            let prefix_start = before.len() - abbr.len();
            if prefix_start == 0 {
                return false;
            }
            let prev_char = before[..prefix_start].chars().next_back();
            match prev_char {
                None => return false,
                Some(c) if !c.is_alphanumeric() => return false,
                Some(c) if is_cjk(c) => return false,
                _ => {}
            }
        }
    }

    true
}

/// Push a sentence if it contains any non-whitespace content.
fn push_sentence(sentences: &mut Vec<SentenceBound>, text: &str, start: usize, end: usize) {
    // Trim leading whitespace from the sentence start.
    let trimmed_start = text[start..end]
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| start + i)
        .unwrap_or(end);

    if trimmed_start < end {
        // Trim trailing whitespace.
        let trimmed_end = text[trimmed_start..end]
            .char_indices()
            .rev()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(i, c)| trimmed_start + i + c.len_utf8())
            .unwrap_or(trimmed_start);

        if trimmed_start < trimmed_end {
            sentences.push(SentenceBound {
                byte_start: trimmed_start,
                byte_end: trimmed_end,
            });
        }
    }
}

/// Build paragraph boundaries from text (split on \n\n or \r\n\r\n).
/// Byte offset just past the blank line that starts at `bytes[newline]`, or
/// None if that newline ends a single line rather than a paragraph.
///
/// A blank line is two terminators in a row, and either may be "\r\n" or "\n".
/// `bytes[newline]` must be the "\n" of the first terminator.
///
/// One definition, because four copies cost four rounds of defects: a bare
/// "\n\n" test dead on CRLF, a slice that carried its own "\r", a swap that
/// lost the mixed form, and paragraph bounds that no sentence bound matched.
/// Each was a copy of this rule somebody had already fixed elsewhere.
pub(crate) fn blank_line_end(bytes: &[u8], newline: usize) -> Option<usize> {
    let mut j = newline + 1;
    if bytes.get(j) == Some(&b'\r') {
        j += 1;
    }
    (bytes.get(j) == Some(&b'\n')).then_some(j + 1)
}

fn build_paragraphs(text: &str) -> Vec<ParagraphBound> {
    let mut result = Vec::new();
    let mut prev = 0;
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // A blank line is two terminators in a row, and either may be "\r\n" or
        // "\n". Matching only the two pure forms missed a document that mixes
        // them around one blank line, which is what a patch or a merge
        // produces, and the paragraph then swallowed the rest of the file.
        if bytes[i] == b'\n' {
            if let Some(next) = blank_line_end(bytes, i) {
                // Cut before the "\r" a CRLF terminator puts here, so the
                // paragraph does not carry its own line ending.
                let end = i - usize::from(text[..i].ends_with('\r'));
                push_paragraph(&mut result, text, prev, end);
                prev = next;
                i = prev;
                continue;
            }
        }
        i += 1;
    }

    // Trailing paragraph.
    if prev < text.len() {
        push_paragraph(&mut result, text, prev, text.len());
    }

    result
}

fn push_paragraph(paragraphs: &mut Vec<ParagraphBound>, text: &str, start: usize, end: usize) {
    let slice = &text[start..end];
    if slice.chars().any(|c| !c.is_whitespace()) {
        paragraphs.push(ParagraphBound {
            byte_start: start,
            byte_end: end,
        });
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x4E00..=0x9FFF |  // CJK Unified Ideographs
        0x3400..=0x4DBF |  // CJK Extension A
        0x2E80..=0x2EFF |  // CJK Radicals Supplement
        0x3000..=0x303F |  // CJK Symbols and Punctuation
        0xF900..=0xFAFF    // CJK Compatibility Ideographs
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(text: &str) -> BoundaryIndex {
        BoundaryIndex::build(text, &[])
    }

    #[test]
    fn cjk_sentence_split() {
        let text = "你好。世界！測試？完成";
        let idx = bounds(text);
        assert_eq!(idx.sentences.len(), 4);
        assert_eq!(
            &text[idx.sentences[0].byte_start..idx.sentences[0].byte_end],
            "你好。"
        );
        assert_eq!(
            &text[idx.sentences[1].byte_start..idx.sentences[1].byte_end],
            "世界！"
        );
        assert_eq!(
            &text[idx.sentences[2].byte_start..idx.sentences[2].byte_end],
            "測試？"
        );
        assert_eq!(
            &text[idx.sentences[3].byte_start..idx.sentences[3].byte_end],
            "完成"
        );
    }

    #[test]
    fn semicolon_splits_sentence() {
        let text = "前半句；後半句。";
        let idx = bounds(text);
        assert_eq!(idx.sentences.len(), 2);
        assert_eq!(
            &text[idx.sentences[0].byte_start..idx.sentences[0].byte_end],
            "前半句；"
        );
        assert_eq!(
            &text[idx.sentences[1].byte_start..idx.sentences[1].byte_end],
            "後半句。"
        );
    }

    #[test]
    fn paragraph_break_splits_sentence() {
        let text = "第一段\n\n第二段";
        let idx = bounds(text);
        assert_eq!(idx.sentences.len(), 2);
        assert_eq!(idx.paragraphs.len(), 2);
    }

    #[test]
    fn latin_sentence_with_abbreviation() {
        let text = "Mr. Smith went home. He is here.";
        let idx = bounds(text);
        // "Mr." should NOT split. "home." should split. "here." should end.
        assert_eq!(idx.sentences.len(), 2);
        assert!(idx.sentences[0].byte_end <= text.find("He").unwrap());
    }

    #[test]
    fn latin_sentence_splits_across_multiple_whitespaces() {
        // cubic review: must handle multiple spaces / tabs / newlines between
        // the terminator and the next capital letter.
        let text = "Alice went home.  Bob followed.";
        let idx = bounds(text);
        assert_eq!(idx.sentences.len(), 2);

        let text = "One ended.\n\nTwo started.";
        let idx = bounds(text);
        assert!(idx.sentences.len() >= 2);

        let text = "Foo.\tBar is next.";
        let idx = bounds(text);
        assert_eq!(idx.sentences.len(), 2);
    }

    #[test]
    fn mixed_cjk_latin() {
        let text = "這是測試。This is a test. 第二句。";
        let idx = bounds(text);
        assert_eq!(idx.sentences.len(), 3);
    }

    #[test]
    fn cjk_adjacent_abbreviation_not_a_sentence_end() {
        // Codex/Gemini review: abbreviation with CJK preceding char should
        // still be recognized as an abbreviation, not a sentence end.
        let text = "這由Mr. Smith處理過。";
        let idx = bounds(text);
        // "Mr." should NOT split. Whole thing is one sentence.
        assert_eq!(idx.sentences.len(), 1);
    }

    #[test]
    fn exclusion_zone_breaks_sentence() {
        let text = "前面的文字`code`後面的文字。";

        // Simulate exclusion zone over code (bytes for the backtick-wrapped
        // part).
        let code_start = text.find('`').unwrap();
        let code_end = text.rfind('`').unwrap() + 1;
        let excluded = vec![ByteRange {
            start: code_start,
            end: code_end,
        }];
        let idx = BoundaryIndex::build(text, &excluded);
        // Should have at least 2 sentence fragments.
        assert!(idx.sentences.len() >= 2);
    }

    #[test]
    fn empty_text() {
        let idx = bounds("");
        assert!(idx.sentences.is_empty());
        assert!(idx.paragraphs.is_empty());
    }

    #[test]
    fn whitespace_only() {
        let idx = bounds("   \n\n   ");
        assert!(idx.sentences.is_empty());
    }

    #[test]
    fn sentence_at_lookup() {
        let text = "第一句。第二句。";
        let idx = bounds(text);
        let s1_mid = text.find('一').unwrap();
        let found = idx.sentence_at(s1_mid);
        assert!(found.is_some());
        assert_eq!(found.unwrap().byte_start, 0);
    }

    #[test]
    fn paragraph_at_lookup() {
        let text = "段落一\n\n段落二";
        let idx = bounds(text);
        let p2_start = text.rfind('段').unwrap();
        let found = idx.paragraph_at(p2_start);
        assert!(found.is_some());
        assert!(found.unwrap().byte_start > 0);
    }

    #[test]
    fn sentences_in_paragraph() {
        let text = "第一句。第二句。\n\n第三句。";
        let idx = bounds(text);
        assert_eq!(idx.paragraphs.len(), 2);
        let sents = idx.sentence_slice(&idx.paragraphs[0]);
        assert_eq!(sents.len(), 2);
        let sents2 = idx.sentence_slice(&idx.paragraphs[1]);
        assert_eq!(sents2.len(), 1);
    }

    #[test]
    fn crlf_paragraph_break() {
        let text = "段落一\r\n\r\n段落二";
        let idx = bounds(text);
        assert_eq!(idx.paragraphs.len(), 2);
    }

    #[test]
    fn crlf_paragraph_break_splits_sentence() {
        let text = "第一段\r\n\r\n展望未來";
        let idx = bounds(text);
        assert_eq!(idx.sentences.len(), 2);
        assert_eq!(idx.paragraphs.len(), 2);

        let first_para_sents = idx.sentence_slice(&idx.paragraphs[0]);
        let second_para_sents = idx.sentence_slice(&idx.paragraphs[1]);
        assert_eq!(first_para_sents.len(), 1);
        assert_eq!(second_para_sents.len(), 1);
    }

    // Paragraph bounds and sentence bounds have to agree on where a blank line
    // is, or "sentence_slice" returns nothing for a paragraph that plainly has
    // content. Three separate splitters answer this question, and each one has
    // been wrong about a different terminator combination in turn, so the
    // property is asserted across all four rather than the pair that happened
    // to work.
    #[test]
    fn every_blank_line_form_agrees_across_paragraphs_and_sentences() {
        for sep in ["\n\n", "\r\n\r\n", "\n\r\n", "\r\n\n"] {
            for (a, b) in [
                ("第一段內容", "第二段內容"),
                ("第一句。第二句。", "第三句。"),
            ] {
                let doc = format!("{a}{sep}{b}");
                let idx = BoundaryIndex::build(&doc, &[]);
                assert_eq!(idx.paragraphs.len(), 2, "paragraphs for {sep:?} in {doc:?}");
                for para in &idx.paragraphs {
                    assert!(
                        !idx.sentence_slice(para).is_empty(),
                        "no sentence in a paragraph with content, {sep:?} in {doc:?}"
                    );
                }
            }
        }
    }
}
