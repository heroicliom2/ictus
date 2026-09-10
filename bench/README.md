# Benchmark harness (phase 0)

Per [docs/roadmap.md](../docs/roadmap.md), this exists **before any kernel
code** so that pillar 1 (speed/lightweight — see
[docs/decisions.md](../docs/decisions.md) D11) is a measured, continuously
tracked property from day one, not an assumed one. Right now (phase 0/1)
there's no Ictus binary to benchmark yet — this harness runs real open
designs through reference simulators (Icarus Verilog, and later
Verilator) and checks they actually produce correct results, so once
Ictus exists there's an established pass/fail + timing baseline to compare
it against, on real designs instead of synthetic ones.

## Layout

- `designs/` — real open hardware designs, each vendored in as a git
  submodule (not copied in directly, so licensing/attribution/updates stay
  with the upstream project).
- `adapters/` — one script per design, `adapters/<name>.sh`. A design's
  build/test process is never uniform (different toolchains, different
  pass/fail signaling), so each adapter owns the specifics of *its*
  design: build it, run it, and exit 0 only if the design's own
  correctness check actually passed (not just "the simulator didn't
  crash" — those aren't the same thing; see the comment in
  `adapters/picorv32.sh` for a concrete example of why).
- `run.sh` — the top-level harness. Discovers every adapter, runs each,
  times it, and prints a summary table.

## Running it

From WSL (see [docs/development.md](../docs/development.md) for why WSL):

```bash
cd /mnt/c/Users/Musa/Desktop/Ictus
bench/run.sh
```

## Adding a design

1. `git submodule add <repo-url> bench/designs/<name>`
2. Write `bench/adapters/<name>.sh`: build the design's own existing
   test/regression setup (don't invent a new one — reuse whatever
   correctness check the upstream project already ships with), and exit
   non-zero unless that check actually reports success. Look at what the
   design's own README/Makefile already uses for "did this pass" before
   assuming a simulator's exit code alone means anything — it usually
   doesn't (a simulator can exit 0 after a design produces the wrong
   answer; `$finish` just means the simulation ended, not that it ended
   correctly).
3. `chmod +x bench/adapters/<name>.sh`, confirm `bench/run.sh` picks it up.

## Current designs

- **picorv32** ([YosysHQ/picorv32](https://github.com/YosysHQ/picorv32),
  ISC license) — a small, self-contained RV32IM(C) CPU core. Chosen first
  because it's small, widely used as a reference design in the hobbyist
  HDL space, and ships its own firmware-based regression self-test
  (prints `"ALL TESTS PASSED."` on success) rather than needing one
  invented for it.

Per docs/roadmap.md phase 0, 3-5 designs is the eventual target (Ibex,
CV32E40P/Rocket were the other candidates raised) — add more as needed
rather than blocking on all of them up front; one working, trustworthy
adapter is worth more than several unverified ones.
