//! Content-level comparison: side-by-side line diff for text, digest card
//! for binary data.

use checksum::{new_hasher, ChecksumAlgorithm};
use imara_diff::intern::InternedInput;
use imara_diff::{Algorithm, Sink};

/// An `imara-diff` sink that collects the edit script as range pairs.
#[derive(Default)]
struct HunkSink {
    hunks: Vec<(std::ops::Range<u32>, std::ops::Range<u32>)>,
}

impl Sink for HunkSink {
    type Out = Vec<(std::ops::Range<u32>, std::ops::Range<u32>)>;

    fn process_change(&mut self, before: std::ops::Range<u32>, after: std::ops::Range<u32>) {
        self.hunks.push((before, after));
    }

    fn finish(mut self) -> Self::Out {
        self.hunks.sort_by_key(|(before, _)| before.start);
        self.hunks
    }
}

/// Digest information about one blob, for the binary comparison card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobInfo {
    pub size: u64,
    pub md5: String,
}

impl BlobInfo {
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = new_hasher(ChecksumAlgorithm::Md5);
        hasher.update(bytes);
        Self {
            size: bytes.len() as u64,
            md5: hasher.finalize_hex(),
        }
    }
}

/// The outcome of comparing two byte buffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentDiff {
    /// Both sides are byte-identical.
    Same,
    /// Text on at least one side: the line-oriented side-by-side diff.
    Text(Vec<DiffLine>),
    /// Binary on either side: only digests are compared.
    Binary { base: BlobInfo, working: BlobInfo },
}

/// One rendered row of a side-by-side diff. Left and right are `None` when
/// the row has no counterpart on that side (pure insertion or deletion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub left: Option<(u32, String)>,
    pub right: Option<(u32, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    /// An edit whose sides are unrelated (paired hunk overflow).
    Changed,
}

/// Heuristic text detection: rejects on NUL bytes, accepts valid UTF-8,
/// falls back to a printable-ratio check for other encodings (e.g. GBK).
pub fn is_probably_text(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    if std::str::from_utf8(bytes).is_ok() {
        return true;
    }
    // Latin-1 / GBK style single-byte content: mostly printable bytes means
    // we render it lossy rather than showing a digest card.
    if bytes.is_empty() {
        return true;
    }
    let printable = bytes
        .iter()
        .filter(|&&b| (0x20..0x7f).contains(&b) || b == b'\n' || b == b'\r' || b == b'\t' || b >= 0x80)
        .count();
    printable * 100 / bytes.len() >= 90
}

/// Compares two blobs. Equal buffers short-circuit; text buffers are split
/// into lines and diffed with the histogram algorithm; everything else
/// degrades to an MD5 digest card. Oversized buffers skip line diffing.
pub fn compare_bytes(base: &[u8], working: &[u8]) -> ContentDiff {
    if base == working {
        return ContentDiff::Same;
    }
    const MAX_LINE_DIFF_BYTES: usize = 4 * 1024 * 1024;
    if base.len() > MAX_LINE_DIFF_BYTES || working.len() > MAX_LINE_DIFF_BYTES {
        return ContentDiff::Binary {
            base: BlobInfo::of(base),
            working: BlobInfo::of(working),
        };
    }
    if !is_probably_text(base) || !is_probably_text(working) {
        return ContentDiff::Binary {
            base: BlobInfo::of(base),
            working: BlobInfo::of(working),
        };
    }

    let base_text = String::from_utf8_lossy(base);
    let working_text = String::from_utf8_lossy(working);
    let base_lines = split_lines(&base_text);
    let working_lines = split_lines(&working_text);
    if base_lines.len() >= u32::MAX as usize || working_lines.len() >= u32::MAX as usize {
        return ContentDiff::Binary {
            base: BlobInfo::of(base),
            working: BlobInfo::of(working),
        };
    }
    let input = InternedInput::new(
        imara_diff::sources::lines_with_terminator(&base_text),
        imara_diff::sources::lines_with_terminator(&working_text),
    );
    let hunks = imara_diff::diff(Algorithm::Histogram, &input, HunkSink::default());
    ContentDiff::Text(render_side_by_side(&base_lines, &working_lines, &hunks))
}

/// Splits into lines, keeping the newline on each line so reconstructed
/// rows round-trip.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find('\n') {
        let (line, tail) = rest.split_at(pos + 1);
        lines.push(line);
        rest = tail;
    }
    if !rest.is_empty() {
        lines.push(rest);
    }
    lines
}

/// Pairs equal regions and hunks into side-by-side rows. Within a hunk,
/// removals and additions are zipped row-wise; the shorter side is padded
/// with `None` and surplus rows are marked [`LineKind::Changed`].
fn render_side_by_side(
    base_lines: &[&str],
    working_lines: &[&str],
    hunks: &[(std::ops::Range<u32>, std::ops::Range<u32>)],
) -> Vec<DiffLine> {
    let mut rows = Vec::new();
    let mut base_pos: u32 = 0;
    let mut working_pos: u32 = 0;

    let flush_context = |rows: &mut Vec<DiffLine>,
                             base_pos: &mut u32,
                             working_pos: &mut u32,
                             until_base: u32,
                             until_working: u32| {
        let common = (until_base - *base_pos).min(until_working - *working_pos);
        for _ in 0..common {
            let text = base_lines[*base_pos as usize].to_string();
            rows.push(DiffLine {
                kind: LineKind::Context,
                left: Some((*base_pos + 1, text.clone())),
                right: Some((*working_pos + 1, text)),
            });
            *base_pos += 1;
            *working_pos += 1;
        }
        // Unequal tails (should not happen when both regions precede a
        // hunk, but stay safe): render the remainder as changed rows.
        while *base_pos < until_base || *working_pos < until_working {
            let left = (*base_pos < until_base).then(|| {
                let row = (*base_pos + 1, base_lines[*base_pos as usize].to_string());
                *base_pos += 1;
                row
            });
            let right = (*working_pos < until_working).then(|| {
                let row = (*working_pos + 1, working_lines[*working_pos as usize].to_string());
                *working_pos += 1;
                row
            });
            rows.push(DiffLine {
                kind: LineKind::Changed,
                left,
                right,
            });
        }
    };

    for (before, after) in hunks {
        flush_context(&mut rows, &mut base_pos, &mut working_pos, before.start, after.start);

        let removed = (before.end - before.start) as usize;
        let added = (after.end - after.start) as usize;
        let paired = removed.min(added);
        for i in 0..paired {
            let left = (
                before.start + i as u32 + 1,
                base_lines[before.start as usize + i].to_string(),
            );
            let right = (
                after.start + i as u32 + 1,
                working_lines[after.start as usize + i].to_string(),
            );
            rows.push(DiffLine {
                kind: LineKind::Changed,
                left: Some(left),
                right: Some(right),
            });
        }
        for i in paired..removed {
            rows.push(DiffLine {
                kind: LineKind::Removed,
                left: Some((
                    before.start + i as u32 + 1,
                    base_lines[before.start as usize + i].to_string(),
                )),
                right: None,
            });
        }
        for i in paired..added {
            rows.push(DiffLine {
                kind: LineKind::Added,
                left: None,
                right: Some((
                    after.start + i as u32 + 1,
                    working_lines[after.start as usize + i].to_string(),
                )),
            });
        }

        base_pos = before.end;
        working_pos = after.end;
    }

    flush_context(
        &mut rows,
        &mut base_pos,
        &mut working_pos,
        base_lines.len() as u32,
        working_lines.len() as u32,
    );
    rows
}
