//! Cutting a log into the passages `sond ask` searches: one per `##` section.

/// Sections longer than this many words are split at paragraph boundaries,
/// so that no passage outgrows what the embedding model reads.
pub const MAX_WORDS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// 1-based line where the chunk starts: its section's `##` heading, or
    /// its first paragraph for the later pieces of a split section.
    pub line: usize,
    /// The section heading, without the `## `.
    pub heading: String,
    /// What gets embedded: `# <title>`, `## <heading>`, a blank line, the text.
    pub text: String,
}

/// The chunks of a log whose title is `title`. Text before the first `##`
/// heading and sections with nothing in them are left out. A `##` inside a
/// fenced code block is not a heading.
pub fn chunks(title: &str, content: &str) -> Vec<Chunk> {
    sections(content)
        .into_iter()
        .flat_map(|s| split(title, s))
        .collect()
}

struct Section<'a> {
    line: usize,
    heading: &'a str,
    /// Paragraphs as (1-based line of their first line, their lines).
    paragraphs: Vec<(usize, Vec<&'a str>)>,
}

fn sections(content: &str) -> Vec<Section<'_>> {
    let mut sections: Vec<Section> = Vec::new();
    let mut fenced = false;
    let mut in_paragraph = false;
    for (i, line) in content.lines().enumerate() {
        let n = i + 1;
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        if !fenced && let Some(heading) = line.strip_prefix("## ") {
            sections.push(Section {
                line: n,
                heading: heading.trim(),
                paragraphs: Vec::new(),
            });
            in_paragraph = false;
        } else if let Some(section) = sections.last_mut() {
            // A `---` rule (as `poke` leaves) separates, it says nothing.
            if matches!(line.trim(), "" | "---") {
                in_paragraph = false;
            } else if in_paragraph {
                section.paragraphs.last_mut().unwrap().1.push(line);
            } else {
                section.paragraphs.push((n, vec![line]));
                in_paragraph = true;
            }
        }
    }
    sections
}

/// Packs a section's paragraphs, in order, into as few chunks as fit in
/// [`MAX_WORDS`]; a paragraph longer than that is a chunk of its own.
fn split(title: &str, section: Section) -> Vec<Chunk> {
    let mut pieces: Vec<Vec<(usize, Vec<&str>)>> = Vec::new();
    let mut words = 0;
    for paragraph in section.paragraphs {
        let n: usize = paragraph
            .1
            .iter()
            .map(|l| l.split_whitespace().count())
            .sum();
        match pieces.last_mut() {
            Some(piece) if words + n <= MAX_WORDS => {
                piece.push(paragraph);
                words += n;
            }
            _ => {
                pieces.push(vec![paragraph]);
                words = n;
            }
        }
    }

    pieces
        .into_iter()
        .enumerate()
        .map(|(i, piece)| Chunk {
            line: if i == 0 { section.line } else { piece[0].0 },
            heading: section.heading.to_string(),
            text: format!(
                "# {title}\n## {}\n\n{}",
                section.heading,
                piece
                    .iter()
                    .map(|(_, lines)| lines.join("\n"))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            ),
        })
        .collect()
}
