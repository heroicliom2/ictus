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
4. [glossary.md](glossary.md) — every HDL/EDA term *and* every
   software/compiler-engineering term (JIT, Cranelift, dataflow graph,
   licensing terms, etc.) used anywhere in these docs, explained in full,
   assuming an electrical-engineering background but no compiler-theory
   background. This project is being built by and for electrical
   engineers — if a doc uses a term without explaining it well enough,
   that's a documentation bug, not something the reader is expected to
   already know. Other docs give a short inline gloss on first use and
   point here for the complete explanation.
5. [development.md](development.md) — toolchain setup and build commands.

The root [README.md](../README.md) is the short public-facing pitch. These
docs are the long-form working memory.

## Keeping this current

When a decision gets made or reversed in conversation, it should land in
`decisions.md` before the session ends — that file is the whole point of
this folder. Stale docs are worse than no docs, since they cause a future
agent to redo settled work or contradict a choice made for a reason it can't
see.

## Writing style for this project

The audience is an electrical engineer, not a compiler/software engineer —
that's who's building this and who it's for. When adding to these docs:
assume strong HDL/digital-design/verification background, and assume
*no* compiler-theory or general software-engineering background. Any
software/compiler-engineering term (JIT, IR, dataflow graph, SIMD,
whatever) gets a short plain-language gloss inline on first use in a given
doc, plus a full explanation added to `glossary.md` if it isn't already
there. Don't assume a term is "obvious" because it's common in software
engineering generally — it isn't obvious to this audience, and that's the
whole point of writing it this way.
