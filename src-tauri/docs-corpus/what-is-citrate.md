# What is Citrate

Citrate is a network for running artificial intelligence with provenance and human
control. It pairs an AI-native layer-1 blockchain with a set of desktop and web
applications so that models, data, compute, and agents can be owned, governed, and
audited by the people who use them rather than by a single platform.

The chain is a BlockDAG that orders work with GhostDAG consensus, so many blocks can
be produced in parallel and later reconciled into one agreed history. Its chain id is
**40204** and its native token is **SALT**. The chain runs an EVM-compatible execution
layer, so ordinary smart contracts and tooling work, while the surrounding protocol
adds the parts a general chain leaves out: identity, compute accounting, model
provenance, and settlement for cooperative training.

Three ideas run through everything Citrate builds:

- **Provenance.** Where a model came from, what data shaped it, and which contributions
  earned a reward are recorded so they can be checked, not merely claimed.
- **Human control.** Software acts under a named person. High-consequence actions stop
  and ask a human before they happen. This is the HIC model (see "Human In Control").
- **Local first.** Your keys, your model, and your memory live on your own device by
  default. The network is something you connect to, not something you hand your data to.

Citrate is a federation of repositories and applications rather than one monolith. The
desktop full node (see "The Citrate desktop node") is the front door: it runs a node,
holds your wallet, serves a local model, keeps your memory graph, and hosts your groups.
