#!/usr/bin/env bash
# Builds and runs PicoRV32's own regression testbench: a firmware
# self-test (bench/designs/picorv32/tests/*.S + firmware/*.c), compiled
# with a RISC-V cross-compiler, run through Icarus Verilog.
#
# PicoRV32's testbench.v prints "ALL TESTS PASSED." when the firmware's
# own checks succeed, and "TIMEOUT" if the design hangs (see
# testbench.v). Icarus/vvp exit 0 in *both* cases -- `$finish` just means
# the simulation ended, not that the design under test behaved correctly
# -- so this script greps the actual output instead of trusting the
# simulator's exit code alone. This distinction is the whole reason
# adapters are per-design rather than one generic "did it exit 0" check
# in run.sh: what "passed" even means is defined by each design's own
# testbench, not by the simulator.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../designs/picorv32" || exit 1

make clean >/dev/null 2>&1
output="$(make TOOLCHAIN_PREFIX=riscv64-unknown-elf- test 2>&1)"
build_status=$?

echo "$output"

if [ "$build_status" -ne 0 ]; then
    echo "picorv32: build/run failed (exit $build_status)"
    exit 1
fi

if ! grep -q "ALL TESTS PASSED\." <<< "$output"; then
    echo "picorv32: simulation completed but did not report ALL TESTS PASSED."
    exit 1
fi
