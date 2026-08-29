# The Citrate desktop node

citrate-core is the Citrate desktop application: a full node and a home for everything
you do on the network, in one program you run on your own machine.

It bundles several things that usually live apart:

- **A full node.** It runs a real Citrate node, syncs the chain from genesis, and reads
  live state directly rather than trusting a remote server. When it cannot reach the
  chain it says so honestly instead of showing stale or invented data.
- **A wallet and custody.** Your signing key is generated on the device and sealed in the
  operating system keyring. It is never exported and never printed. Signing happens only
  through the SignatureCeremony (see "Keys and the ceremony").
- **A local model.** A language model runs on your own hardware for chat and for the
  agent, so ordinary use needs no external inference service.
- **Memory.** A private, encrypted knowledge graph is served locally and grows as you
  work (see "Your memory graph").
- **Groups.** Encrypted group workspaces, private peer-to-peer clusters, and cooperative
  training all run from the same app (see "Groups and the Commons").

The app is organized around two questions: **you** (your identity, wallet, node, and
memory) and **your groups** (the people and agents you work with). A non-technical person
should be able to install it and reach a working state — a model running, in a group —
without reading a manual.

Under the hood, heavier components run as supervised sidecar processes that the app
starts, watches, and restarts if they crash, each spoken to over a local socket. If a
sidecar is missing or down, the feature that depends on it degrades honestly rather than
pretending to work.
