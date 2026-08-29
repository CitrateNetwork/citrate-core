# Human In Control (HIC)

Citrate governs software with a model called **HIC — Human In Control**. The name is
deliberate: a person holds authority over what software does, rather than being a step in
a loop the machine runs. HIC is enforced in code, not in prompts or policy documents, and
it is graded so that autonomy is earned and bounded rather than all-or-nothing.

The levels:

- **HIC-0 — observe.** The software may watch and report but take no action. New agents
  start here, on probation, cheap to stop.
- **HIC-1 — every action stops for the human.** Each action pauses for a named person to
  decide. For an on-chain action that decision is a signature in the ceremony; for a code
  or shell task it is an explicit approve or reject. Some actions are always HIC-1 — for
  example an intent whose calldata cannot be decoded, or a transfer over a set ceiling.
- **HIC-2 — budgeted autonomy.** A grant gives the agent a bounded envelope: which action
  classes it may take, in what scope, under what budget, and until when. Inside the
  envelope it moves at machine speed; at the edge it stops and asks.
- **HIC-X — ungoverned.** No grant applies. The agent drops to observe-only and is
  effectively quarantined until a human re-authorizes it.

A practical consequence: an approval must happen on the real ceremony or control surface.
A convenient shortcut — say, an "approve" button on a pop-up notification — would be a way
around HIC-1, so Citrate does not offer one. The point of stopping for a human is that a
human actually looks.
