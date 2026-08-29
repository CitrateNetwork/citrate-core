# Keys and the ceremony

Your signing key never leaves your device. It is generated locally, sealed in the
operating system keyring, and used only inside the app. No screen, log, error message, or
network call ever returns the key, the seed, or the entropy behind it.

Every signature — yours today, and any agent's or app's later — goes through one path: the
**SignatureCeremony**. The ceremony is the single human-in-the-loop signing surface, and
the code that can actually sign is reachable only from the ceremony's approve step.
Nothing else in the system can produce a signature.

The ceremony works like this:

1. A request arrives as an **intent**: who is asking, what chain it targets, and the raw
   action to be signed. The origin is shown to you exactly as given, so a request from an
   agent is visibly from that agent, not disguised as coming from you.
2. The intent is **decoded** into a human-readable description of what it authorizes. If
   the action cannot be decoded, approval is blocked until you explicitly acknowledge that
   you are signing something opaque.
3. You **approve or reject** a specific request by its id. There is no "approve the
   latest" and no auto-approve. One approval yields exactly one signature and is then
   consumed.
4. If the wallet is locked, the ceremony fails closed rather than signing.

Because signing is centralized in the ceremony, no sidecar, daemon, agent, or remote
service ever holds your key or signs on its own. An agent can *ask* for a signature by
creating an intent; only you can grant it, and only through the ceremony.
