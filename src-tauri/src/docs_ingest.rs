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

/// A summary of what an ingest run authored (or why it was skipped).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IngestReport {
    pub docs: usize,
    pub chunks: usize,
    /// Present when nothing was ingested: "not-semantic" (no BGE), "already-seeded",
    /// or "empty-corpus". Absent on a real ingest. Keeps the trigger honest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

impl IngestReport {
    pub fn skipped(reason: &str) -> Self {
        IngestReport { docs: 0, chunks: 0, skipped: Some(reason.to_string()) }
    }
}

/// Read a corpus directory of markdown docs into `(title, markdown)` pairs. The
/// title is the doc's first H1 if present, else the file stem. Non-`.md` files are
/// ignored. A missing/empty dir yields an empty list (the honest "nothing to
/// ingest yet" state — the corpus ships empty until curated, Rule 7). Recurses one
/// level so `docs-corpus/guides/*.md` is included.
pub fn read_corpus_dir(dir: &std::path::Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| format!("read {}: {e}", d.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let md = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                let title = first_h1(&md).unwrap_or_else(|| {
                    path.file_stem().and_then(|s| s.to_str()).unwrap_or("doc").to_string()
                });
                out.push((title, md));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0)); // deterministic order (stable ingest)
    Ok(out)
}

fn first_h1(md: &str) -> Option<String> {
    md.lines().find_map(|l| {
        let t = l.trim_start();
        (t.starts_with("# ") && !t.starts_with("##")).then(|| t.trim_start_matches('#').trim().to_string())
    })
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
        skipped: None,
    })
}

/// A stable content hash for a chunk (sha256 hex, first 16 bytes) — the idempotence
/// key. Stable across app versions and rebuilds (sha256, not `DefaultHasher`), so a
/// chunk already authored on a prior run — or in a prior app version's corpus — is
/// NEVER re-asserted. This is what makes a corpus that GROWS across versions
/// (WP1.2's reference packs added to WP1.1's Almanac docs) re-pack only the *new*
/// chunks: monotone, no duplicates (TLA+ `MemoryPack`).
pub fn chunk_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(content.as_bytes());
    hex::encode(&digest[..16])
}

/// Incremental variant of [`ingest_docs`]: author only the chunks whose content hash
/// is NOT already in `seen`, inserting each newly-authored hash. Re-running with the
/// same corpus authors nothing (every hash is already seen); adding docs authors only
/// the new chunks. `author` aborts the run on error WITHOUT recording that chunk's
/// hash, so a failed write is retried on the next run (never silently lost).
pub fn ingest_docs_incremental<F>(
    docs: &[(String, String)],
    max_chars: usize,
    seen: &mut std::collections::BTreeSet<String>,
    mut author: F,
) -> Result<IngestReport, String>
where
    F: FnMut(&DocChunk) -> Result<(), String>,
{
    let mut chunks = 0usize;
    for (title, md) in docs {
        for chunk in chunk_markdown(title, md, max_chars) {
            let h = chunk_hash(&chunk.content);
            if seen.contains(&h) {
                continue; // already authored on a prior run — no dupe
            }
            author(&chunk)?; // on error: abort, do NOT mark seen (retried next run)
            seen.insert(h);
            chunks += 1;
        }
    }
    Ok(IngestReport {
        docs: docs.len(),
        chunks,
        skipped: None,
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
    fn read_corpus_dir_is_empty_and_ok_when_absent_or_empty() {
        // The corpus ships EMPTY until curated (Rule 7) — an absent dir is not an
        // error, it is "nothing to ingest yet".
        let missing = std::path::Path::new("/no/such/corpus/dir/xyz");
        assert_eq!(read_corpus_dir(missing).unwrap().len(), 0);
    }

    #[test]
    fn read_corpus_dir_titles_from_h1_else_filename_and_ignores_non_md() {
        let dir = std::env::temp_dir().join(format!("citrate-corpus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("guides")).unwrap();
        std::fs::write(dir.join("staking.md"), "# Staking Guide\n\nStake 32k.").unwrap();
        std::fs::write(dir.join("no-heading.md"), "just body, no h1").unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored non-md").unwrap();
        std::fs::write(dir.join("guides").join("nested.md"), "# Nested\n\nrecursed").unwrap();

        let docs = read_corpus_dir(&dir).unwrap();
        let titles: Vec<&str> = docs.iter().map(|(t, _)| t.as_str()).collect();
        assert!(titles.contains(&"Staking Guide"), "H1 becomes the title");
        assert!(titles.contains(&"no-heading"), "file stem when no H1");
        assert!(titles.contains(&"Nested"), "recurses one level into subdirs");
        assert!(!titles.iter().any(|t| t.contains("notes")), "non-.md ignored");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_shipped_docs_corpus_is_curated_and_ingestable() {
        // gA-memory: the corpus must SHIP with real docs, or the first-run ingest has nothing to
        // author and the memory graph stays empty (the gate could never flip). This reads the actual
        // bundled `src-tauri/docs-corpus/` and proves it is non-empty and chunks to authorable pieces.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs-corpus");
        let docs = read_corpus_dir(&dir).expect("read shipped corpus");
        assert!(
            docs.len() >= 6,
            "the starter corpus must ship real docs; got {}",
            docs.len()
        );
        let mut total_chunks = 0usize;
        for (title, md) in &docs {
            assert!(!title.trim().is_empty(), "each doc has a title");
            let chunks = chunk_markdown(title, md, 1200);
            assert!(
                !chunks.is_empty(),
                "doc {title:?} produces at least one chunk"
            );
            total_chunks += chunks.len();
        }
        assert!(
            total_chunks >= docs.len(),
            "the corpus produces ingestable chunks"
        );
    }

    #[test]
    fn the_reference_packs_ship_and_are_ingestable() {
        // WP1.2: the corpus must ship the reference knowledge Hermes pre-packs — solc,
        // EVM, Rust, front-end, business-admin, legal — as memory nodes (procedural
        // how-tos stay P3 skills). Each reference doc lives under docs-corpus/reference/
        // and chunks to authorable pieces so first-run ingest packs it.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs-corpus");
        let docs = read_corpus_dir(&dir).expect("read shipped corpus");
        let titles: Vec<String> = docs.iter().map(|(t, _)| t.to_lowercase()).collect();
        for topic in ["solidity", "evm", "rust", "front-end", "business", "legal"] {
            assert!(
                titles.iter().any(|t| t.contains(topic)),
                "reference pack for {topic:?} must ship; titles = {titles:?}"
            );
        }
        // The reference docs alone chunk to real content (not empty stubs).
        let ref_dir = dir.join("reference");
        let ref_docs = read_corpus_dir(&ref_dir).expect("read reference packs");
        assert!(ref_docs.len() >= 6, "six reference packs ship; got {}", ref_docs.len());
        for (title, md) in &ref_docs {
            assert!(!chunk_markdown(title, md, MAX_CHUNK_CHARS).is_empty(), "{title:?} chunks");
        }
    }

    #[test]
    fn skipped_report_carries_a_reason_and_zero_counts() {
        let r = IngestReport::skipped("not-semantic");
        assert_eq!(r.docs, 0);
        assert_eq!(r.chunks, 0);
        assert_eq!(r.skipped.as_deref(), Some("not-semantic"));
    }

    #[test]
    fn chunk_hash_is_stable_and_content_addressed() {
        // Same content → same hash (stable across runs/versions); different → different.
        assert_eq!(chunk_hash("A › S1\n\nbody"), chunk_hash("A › S1\n\nbody"));
        assert_ne!(chunk_hash("A › S1\n\nbody"), chunk_hash("A › S1\n\nother"));
        // 16 bytes → 32 hex chars.
        assert_eq!(chunk_hash("x").len(), 32);
    }

    #[test]
    fn incremental_skips_already_seen_chunks_on_a_second_run() {
        let docs = vec![("A".to_string(), "## S1\nbody one\n\n## S2\nbody two".to_string())];
        let mut seen = std::collections::BTreeSet::new();
        let mut authored: Vec<String> = Vec::new();
        // First run: both chunks authored, both hashes recorded.
        let r1 = ingest_docs_incremental(&docs, MAX_CHUNK_CHARS, &mut seen, |c| {
            authored.push(c.content.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(r1.chunks, 2);
        assert_eq!(seen.len(), 2);
        // Second run over the SAME corpus: nothing authored (monotone, no dupes).
        let before = authored.len();
        let r2 = ingest_docs_incremental(&docs, MAX_CHUNK_CHARS, &mut seen, |c| {
            authored.push(c.content.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(r2.chunks, 0);
        assert_eq!(authored.len(), before, "no chunk re-authored on the second run");
    }

    #[test]
    fn incremental_authors_only_the_new_docs_when_the_corpus_grows() {
        // WP1.2: a later app version ADDS reference packs to the shipped corpus. Only
        // the new docs' chunks are authored; the original docs are not re-packed.
        let v1 = vec![("Almanac".to_string(), "## Intro\nwelcome".to_string())];
        let mut seen = std::collections::BTreeSet::new();
        let r1 = ingest_docs_incremental(&v1, MAX_CHUNK_CHARS, &mut seen, |_c| Ok(())).unwrap();
        assert_eq!(r1.chunks, 1);

        let v2 = vec![
            ("Almanac".to_string(), "## Intro\nwelcome".to_string()), // unchanged
            ("Solidity".to_string(), "## solc\nthe compiler".to_string()), // NEW reference pack
        ];
        let mut newly: Vec<String> = Vec::new();
        let r2 = ingest_docs_incremental(&v2, MAX_CHUNK_CHARS, &mut seen, |c| {
            newly.push(c.breadcrumb.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(r2.chunks, 1, "only the new Solidity chunk is authored");
        assert_eq!(newly, vec!["Solidity › solc"]);
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn incremental_does_not_mark_a_chunk_seen_when_its_author_fails() {
        // A failed write must be RETRIED next run — never silently dropped. So the
        // failing chunk's hash is not recorded, and the run aborts at it.
        let docs = vec![("A".to_string(), "## S1\nx\n\n## S2\ny".to_string())];
        let mut seen = std::collections::BTreeSet::new();
        let mut n = 0;
        let err = ingest_docs_incremental(&docs, MAX_CHUNK_CHARS, &mut seen, |_c| {
            n += 1;
            if n == 2 { Err("daemon write failed".to_string()) } else { Ok(()) }
        })
        .unwrap_err();
        assert_eq!(err, "daemon write failed");
        assert_eq!(seen.len(), 1, "only the successfully-authored chunk is marked seen");
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
