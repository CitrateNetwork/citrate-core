# Your memory graph

Citrate gives you and your agent a shared, private memory: a knowledge graph that grows as
you work. It is often described as "git for agents" — a versioned, inspectable record of
what has been learned, rather than a hidden context window you cannot see.

The memory has a few defining properties:

- **Private and local.** The graph is stored on your device, encrypted with a key held in
  the operating system keyring. A memory daemon serves it locally over a socket; it is not
  a cloud service holding your notes.
- **Semantic recall.** An on-device embedding model lets you and the agent find memories by
  meaning, not just by exact words. Recall, search, and neighborhood queries all run
  against the real local graph.
- **Honest when empty.** A brand-new install has little in it, and the app shows that
  honestly rather than inventing nodes to look full.

On first run, the app preloads a small set of bundled Citrate documentation into a
documentation area of your memory, so the graph starts with something real to explore and
so the agent has grounded material to answer from. This preload runs once, only when the
semantic embedder is available, and skips cleanly if there is nothing new to add — it will
never write a placeholder just to appear populated.

You can see the memory as a constellation: documents and concepts as points, related ones
drawn near each other. As you chat, ingest files, and work in groups, the constellation
fills in, and both you and your agent can navigate it.
