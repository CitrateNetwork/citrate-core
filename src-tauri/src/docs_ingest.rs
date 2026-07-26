//! W3.2 — Citrate docs knowledge preload (mem RAG).
//!
//! Chunks the bundled Citrate documentation corpus and authors each chunk into the
//! member's `citrate-docs` mem tenant via `memory.assert`, so the agent (W3.3 tool
//! loop) can recall grounded Citrate knowledge. Chunking is heading-aware: each
//! chunk carries a `Title › Section` breadcrumb so a recall hit cites its source
//! and the model has context for the fragment.
//!
//! HARD PREREQUISITE — do NOT ingest without it: the mem daemon must have the BGE
//! embedder loaded. `memory.assert` embeds at write time, and an UNEMBEDDED node is
//! invisible to every semantic query, forever and silently. Authoring docs without
//! the embedder would fill the graph with unfindable knowledge — a Rule-1
//! violation dressed as success. The first-run ingest is therefore gated on the
//! embedder being present (the ~440 MB BGE model bundled with mem-mcp; an S7 item).
//! Bundling that model + curating which docs are safe to ship (Rule 7 — no
//! confidential-tier content) are the two things that turn this seam on.

/// The mem tenant the Citrate docs corpus lands in.
pub const DOCS_TENANT: &str = "citrate-docs";

/// Target max characters per chunk. Small enough that several hits fit a prompt
/// budget, large enough to keep a coherent idea together.
pub const MAX_CHUNK_CHARS: usize = 1200;

/// One chunk of documentation ready to author into the mem graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocChunk {
    /// `Title` or `Title › Section` — shown as the recall source.
    pub breadcrumb: String,
    /// What gets asserted: the breadcrumb followed by the section body.
    pub content: String,
}

/// A summary of what an ingest run authored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestReport {
    pub docs: usize,
    pub chunks: usize,
}

/// Split a markdown document into heading-aware chunks. Headings (`#`..`######`)
/// outside fenced code blocks start a new section; each section body is packed into
/// `<= max_chars` pieces at paragraph then word boundaries (UTF-8 safe). Fenced
/// code (```) is kept verbatim and its `#` lines are NOT treated as headings.
pub fn chunk_markdown(doc_title: &str, md: &str, max_chars: usize) -> Vec<DocChunk> {
    let mut out = Vec::new();
    let mut heading = String::new();
    let mut body = String::new();
    let mut in_fence = false;

    for line in md.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            body.push_str(line);
            body.push('\n');
            continue;
        }
        if !in_fence && is_heading(trimmed) {
            // A new heading closes the previous section.
            flush_section(doc_title, &heading, &body, max_chars, &mut out);
            body.clear();
            heading = heading_text(trimmed);
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    flush_section(doc_title, &heading, &body, max_chars, &mut out);
    out
}

/// Chunk a corpus of `(title, markdown)` docs and author each chunk via `author`,
/// which returns `Ok(())` per chunk or an error that aborts the run. Decoupled from
/// the mem transport so it is trivially testable; production passes a closure that
/// calls `MemoryManager::assert(DOCS_TENANT, &chunk.content, "reference")`.
pub fn ingest_docs<F>(
    docs: &[(String, String)],
    max_chars: usize,
    mut author: F,
) -> Result<IngestReport, String>
where
    F: FnMut(&DocChunk) -> Result<(), String>,
{
    let mut chunks = 0usize;
    for (title, md) in docs {
        for chunk in chunk_markdown(title, md, max_chars) {
            author(&chunk)?;
            chunks += 1;
        }
    }
    Ok(IngestReport {
        docs: docs.len(),
        chunks,
    })
}

fn is_heading(trimmed: &str) -> bool {
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ')
}

fn heading_text(trimmed: &str) -> String {
    trimmed.trim_start_matches('#').trim().to_string()
}

fn flush_section(doc_title: &str, heading: &str, body: &str, max_chars: usize, out: &mut Vec<DocChunk>) {
    let body = body.trim();
    if body.is_empty() {
        return;
    }
    let breadcrumb = if heading.is_empty() {
        doc_title.to_string()
    } else {
        format!("{doc_title} › {heading}")
    };
    for piece in pack_to_size(body, max_chars.max(1)) {
        out.push(DocChunk {
            breadcrumb: breadcrumb.clone(),
            content: format!("{breadcrumb}\n\n{piece}"),
        });
    }
}

/// Greedily pack paragraphs (blank-line separated) into `<= max` pieces; a single
/// paragraph over `max` is word-split (never mid-char). Always yields >= 1 piece
/// for non-empty input.
fn pack_to_size(text: &str, max: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut cur = String::new();
    for para in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        if para.chars().count() > max {
            if !cur.is_empty() {
                pieces.push(std::mem::take(&mut cur));
            }
            pieces.extend(word_split(para, max));
            continue;
        }
        let combined = if cur.is_empty() {
            para.chars().count()
        } else {
            cur.chars().count() + 2 + para.chars().count()
        };
        if combined > max && !cur.is_empty() {
            pieces.push(std::mem::take(&mut cur));
        }
        if cur.is_empty() {
            cur.push_str(para);
        } else {
            cur.push_str("\n\n");
            cur.push_str(para);
        }
    }
    if !cur.is_empty() {
        pieces.push(cur);
    }
    pieces
}

/// Split an oversized paragraph on whitespace so no piece exceeds `max` chars
/// (a single word longer than `max` is emitted whole rather than cut mid-char).
fn word_split(para: &str, max: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut cur = String::new();
    for word in para.split_whitespace() {
        let combined = if cur.is_empty() {
            word.chars().count()
        } else {
            cur.chars().count() + 1 + word.chars().count()
        };
        if combined > max && !cur.is_empty() {
            pieces.push(std::mem::take(&mut cur));
        }
        if cur.is_empty() {
            cur.push_str(word);
        } else {
            cur.push(' ');
            cur.push_str(word);
        }
    }
    if !cur.is_empty() {
        pieces.push(cur);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_headings_with_breadcrumbs() {
        let md = "Intro line under the title.\n\n## Staking\nStake 32k SALT.\n\n## Rewards\nRewards accrue per block.";
        let chunks = chunk_markdown("Validator Guide", md, MAX_CHUNK_CHARS);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].breadcrumb, "Validator Guide");
        assert_eq!(chunks[1].breadcrumb, "Validator Guide › Staking");
        assert_eq!(chunks[2].breadcrumb, "Validator Guide › Rewards");
        // The breadcrumb is prepended to the body so a recall hit cites its source.
        assert!(chunks[1].content.starts_with("Validator Guide › Staking\n\n"));
        assert!(chunks[1].content.contains("Stake 32k SALT."));
    }

    #[test]
    fn heading_hashes_inside_code_fences_are_not_headings() {
        let md = "## Config\nUse this:\n\n```toml\n# not a heading\nport = 8080\n```\nDone.";
        let chunks = chunk_markdown("Docs", md, MAX_CHUNK_CHARS);
        // One section (Config); the fenced "# not a heading" stays in the body.
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].breadcrumb, "Docs › Config");
        assert!(chunks[0].content.contains("# not a heading"));
        assert!(chunks[0].content.contains("port = 8080"));
    }

    #[test]
    fn oversized_section_is_packed_under_the_limit() {
        // Three paragraphs of ~40 chars each; max 60 forces multiple pieces.
        let para = "word ".repeat(8); // ~40 chars
        let md = format!("# Big\n\n{para}\n\n{para}\n\n{para}");
        let chunks = chunk_markdown("D", &md, 60);
        assert!(chunks.len() >= 2, "oversized section splits");
        for c in &chunks {
            // Body piece (content minus the breadcrumb line) stays within budget.
            let body = c.content.splitn(2, "\n\n").nth(1).unwrap_or("");
            assert!(body.chars().count() <= 60, "piece within max: {body:?}");
        }
    }

    #[test]
    fn a_single_word_longer_than_max_is_not_cut_mid_char() {
        let md = "# H\n\nsupercalifragilisticexpialidocious";
        let chunks = chunk_markdown("D", md, 5);
        // The long word is emitted whole (never split mid-UTF-8-char).
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("supercalifragilisticexpialidocious"));
    }

    #[test]
    fn empty_or_whitespace_doc_yields_no_chunks() {
        assert!(chunk_markdown("T", "", MAX_CHUNK_CHARS).is_empty());
        assert!(chunk_markdown("T", "   \n\n  \n", MAX_CHUNK_CHARS).is_empty());
        assert!(chunk_markdown("T", "# Heading with no body\n", MAX_CHUNK_CHARS).is_empty());
    }

    #[test]
    fn ingest_docs_authors_every_chunk_and_reports_counts() {
        let docs = vec![
            ("A".to_string(), "## S1\nbody one\n\n## S2\nbody two".to_string()),
            ("B".to_string(), "just a title body".to_string()),
        ];
        let mut authored: Vec<String> = Vec::new();
        let report = ingest_docs(&docs, MAX_CHUNK_CHARS, |c| {
            authored.push(c.content.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(report.docs, 2);
        assert_eq!(report.chunks, 3); // A→2 sections, B→1
        assert_eq!(authored.len(), 3);
        assert!(authored[0].starts_with("A › S1"));
        assert!(authored[2].starts_with("B\n\n"));
    }

    #[test]
    fn ingest_docs_aborts_on_author_error_without_swallowing_it() {
        let docs = vec![("A".to_string(), "## S1\nx\n\n## S2\ny".to_string())];
        let mut n = 0;
        let err = ingest_docs(&docs, MAX_CHUNK_CHARS, |_c| {
            n += 1;
            if n == 2 {
                Err("daemon write failed".to_string())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(err, "daemon write failed");
        assert_eq!(n, 2, "stopped at the failing chunk, not silently continued");
    }
}
