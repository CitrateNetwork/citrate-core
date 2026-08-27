---
created: 2026-08-27T00:00:00Z
branch: main
author: Larry Klosowski (@SaulBuilds) + Claude Opus 4.8
status: essay
---

# Disjoint ownership, or: how to let a dozen branches race to main and never collide

The Commons build had six feature lanes converging on one desktop app. Fourteen-odd pull requests
landed on `main` over three days, out of order, several in the same hour, some while others were
mid-flight — and not one needed a rebase, a merge-conflict resolution, or a "who touched this file"
argument. That isn't luck or discipline. It's a property you can engineer, and the trick is small
enough to write on an index card: **decide, before any code, which lane owns which file, and make a
script fail the branch that strays.**

## The usual failure

The default way many streams share a repo is optimistic: everyone works on `main`-ish branches,
touches whatever they need, and sorts out the collisions at merge time. This scales badly in exactly
the regime we're now in — where the contributors are fast (agents, or humans with agents) and there
are more of them than there are humans to referee. Merge conflicts are the *visible* cost; the
invisible one is worse. Two lanes both edit `store.ts` for unrelated reasons, both pass their own
tests, both merge, and the interaction between their edits is a bug that neither branch could have
caught because neither branch contained both changes. The faster the lanes move, the more often this
happens, and the harder it is to attribute.

## The move

Freeze a spine, then partition the rest. Concretely, for Commons:

1. **Scaffold first.** One lane (and only one) builds the shared surface — the bridge contracts, the
   command registrations, the shell — and freezes it. Every downstream lane fills in behind a frozen
   interface; it never edits the spine.
2. **Write ownership down as data.** A plain file — `cx-ownership.map` — lists, per lane, the globs
   it may touch. `s1 src-tauri/src/model.rs`, `s2 src-tauri/src/storage.rs`, and so on. Disjoint by
   construction. Test files, slices, surfaces — each assigned to exactly one lane.
3. **Make it mechanical.** `cx-ownership-check.sh <lane>` diffs the branch against `main` and fails
   if any changed path falls outside that lane's set. Run it before every commit; wire it into CI.

Now the guarantee is structural, not behavioural. Two lanes *cannot* edit the same file, so their
merges commute — any order is fine, because their diffs are literally disjoint. The referee is a
twenty-line shell script instead of a person reading every PR.

## What it costs, and what it doesn't buy

It costs foresight. You have to design the seam before you build the feature — decide that models
lives in `model.rs`/`serve.rs`/`ai.rs`, that storage is stateless-per-command so it needs no shared
state, that the comms relay is a process-wide singleton so it never touches the s0-owned `lib.rs`.
Some of those were real design decisions forced early by the ownership map, and they were *better*
decisions for being forced early. When a lane genuinely needed to touch shared state, the map made
that visible as a serialized "spine PR" instead of a silent edit — the friction was the point.

And it's worth being precise about what it does not buy: **it prevents races, not bugs.** Disjoint
ownership guarantees two lanes won't stomp each other's files. It says nothing about whether the code
in each file is correct. The same three days that produced zero merge conflicts also produced a
money-path change that was four-layers-of-green and still wrong — and no ownership map would have
caught that, because it lived entirely inside one lane's own files. The scaffold buys you the right to
go fast in parallel. Catching the lie is a different tool, and you still have to read the contract.

## The idea worth carrying

As the number of fast contributors per human keeps climbing, the coordination cost of a shared
codebase becomes the bottleneck, and "sort it out at merge time" stops scaling. Disjoint file
ownership, enforced mechanically, converts that coordination from a runtime negotiation into a
compile-time-ish invariant. It is cheap, it is boring, and it is the difference between a dozen
agents building one thing and a dozen agents fighting over one thing.
