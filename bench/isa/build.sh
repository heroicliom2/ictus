#!/usr/bin/env bash
# Assembles picorv32's own per-instruction tests (vendored from
# riscv-tests' rv32ui suite) into flat memory images for Ictus's
# differential test, crates/ictus-cli/tests/differential_picorv32_isa.rs.
#
# The images are committed, so running the test suite needs no RISC-V
# toolchain -- only regenerating them does. Rerun this after changing
# start.S, link.ld, or the set of tests.
#
# Built for picorv32's *default* configuration, which is the only one
# Ictus can run (it has no way to override a top-level parameter yet): the
# base RV32I instruction set, so the multiply, divide and remainder tests
# are excluded -- they need ENABLE_MUL/ENABLE_DIV, whose units are separate
# modules picorv32 instantiates.
#
# Output format: one 32-bit little-endian word per line in hex, which is
# what the testbench's `$readmemh` into a 32-bit-wide memory expects.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../.."
tests="$root/bench/designs/picorv32/tests"
out="$root/crates/ictus-cli/tests/fixtures/picorv32_isa"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

CC="${CC:-riscv64-unknown-elf-gcc}"
OBJCOPY="${OBJCOPY:-riscv64-unknown-elf-objcopy}"

mkdir -p "$out"
rm -f "$out"/*.hex

for src in "$tests"/*.S; do
	name="$(basename "$src" .S)"
	case "$name" in
		mul*|div*|rem*) continue ;;
	esac

	"$CC" -march=rv32i -mabi=ilp32 -nostdlib -nostartfiles -Wl,--no-relax \
		-T "$here/link.ld" \
		-DTEST_FUNC_NAME=mytest -DTEST_FUNC_TXT="\"$name\"" -DTEST_FUNC_RET=mytest_ret \
		-I "$tests" \
		-o "$work/$name.elf" "$here/start.S" "$src"
	"$OBJCOPY" -O binary "$work/$name.elf" "$work/$name.bin"
	od -An -tx4 -v -w4 "$work/$name.bin" | tr -d ' ' > "$out/$name.hex"
done

echo "built $(ls "$out"/*.hex | wc -l) images into $out"
wc -l "$out"/*.hex | sort -n | tail -3
