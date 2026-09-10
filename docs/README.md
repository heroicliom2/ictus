# Ictus documentation

This folder exists so the project can be picked up cold — by a human or an
agent — with no prior context beyond what's written here. If you're starting
a session on this repo, read in this order:

1. [architecture.md](architecture.md) — what we're building and why it's
   structured this way.
2. [decisions.md](decisions.md) — choices already made and rejected, with
   rationale. Read this before proposing an alternative to something that
   looks arbitrary — it's probably already been considered.
3. [roadmap.md](roadmap.md) — the phased plan and what "done" means per
   phase.
4. [glossary.md](glossary.md) — HDL/EDA terminology used throughout.
5. [development.md](development.md) — toolchain setup and build commands.

The root [README.md](../README.md) is the short public-facing pitch. These
docs are the long-form working memory.

## Keeping this current

When a decision gets made or reversed in conversation, it should land in
`decisions.md` before the session ends — that file is the whole point of
this folder. Stale docs are worse than no docs, since they cause a future
agent to redo settled work or contradict a choice made for a reason it can't
see.
