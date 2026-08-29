# Groups and the Commons

Beyond running your own node, Citrate lets you work with other people and agents in
**groups**. A group is a private space you create and control, and the people and agents in
it are its members. The set of features built on the group primitive is called the Commons.

A group can do several things, and each is built to keep the group's data in the group:

- **Talk.** Group and one-to-one messaging is end-to-end encrypted. The relay that passes
  messages between members never holds a key and cannot read the contents — it is
  server-blind by design. When someone leaves, the group's shared secret is rotated so
  they lose access going forward.
- **Share.** A group's members can form a private peer-to-peer cluster and share a set of
  files among themselves, pinned so they stay available to the group.
- **Train.** A group can run a round of cooperative ("federated") model training, where
  members contribute compute and data without pooling raw data in one place. Contributions
  are metered and settled on-chain so rewards follow real work.

Roles and permissions are enforced where the data actually flows, not just in the interface
— an owner or admin's rights are checked at the relay, so the rules hold even against a
modified client.

Groups are governed the same way everything else is. An agent acting in a group is a member
under the HIC model, and any on-chain effect a group action produces — a settlement, a pin
bond — surfaces as a human-approved ceremony. The group moves quickly on the safe parts and
stops for a person on the consequential ones.
