---
created: 2026-10-01
branch: hup/n4-corpus
author: Larry Klosowski + Claude Opus 5.5
status: test fixture
---

# Fixture knowledge corpus (HUP-S3.1)

A byte-exact copy of the citrate-memories `mem-corpus` golden fixture
(`crates/mem-corpus/tests/fixtures/bundle`, built from that crate's fixture spec).
It is small, synthetic test content, not the release corpus. The importer reads only
`manifest.json`, `skills.lock` and `tenants/`; this README is not part of the corpus.
