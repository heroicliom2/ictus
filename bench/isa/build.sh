#!/usr/bin/env bash
# Assembles picorv32's own per-instruction tests (vendored from
# riscv-tests' rv32ui suite) into flat memory images for Ictus's
# differential test, crates/ictus-cli/tests/differential_picorv32_isa.rs.
#
# The images are committed, so running the test suite needs no RISC-V
# toolchain -- only regenerating them does. Rerun this after changing
# start.S, link.ld, or the set of tests.
#
# Two sets are built from the same sources:
#
#   picorv32_isa/    -march=rv32i   base instructions only
#   picorv32_isa_c/  -march=rv32ic  the assembler also emits compressed
#                                   (16-bit) instructions wherever it can,
#                                   about half of them
#   picorv32_isa_m/  -march=rv32im  the multiply/divide/remainder tests
#                                   only, which need ENABLE_MUL/ENABLE_DIV
#
# The second set exists because a picorv32 configured with COMPRESSED_ISA
# runs plain rv32i code with a bus trace *identical* to the default's --
# measured, not assumed -- so without compressed instructions in the
# program, the compressed-instruction decoder never runs at all.
#
# The multiply, divide and remainder tests go only in the third set: they
# need ENABLE_MUL/ENABLE_DIV, whose units are separate modules picorv32
# instantiates.
#
# Output format: one 32-bit little-endian word per line in hex, which is
# what the testbench's `$readmemh` into a 32-bit-wide memory expects.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../.."
tests="$root/bench/designs/picorv32/tests"
fixtures="$root/crates/ictus-cli/tests/fixtures"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

CC="${CC:-riscv64-unknown-elf-gcc}"
OBJCOPY="${OBJCOPY:-riscv64-unknown-elf-objcopy}"

build_set() {
	local march="$1" out="$2" want="$3"
	mkdir -p "$out"
	rm -f "$out"/*.hex

	for src in "$tests"/*.S; do
		local name
		name="$(basename "$src" .S)"
		case "$want:$name" in
			base:mul*|base:div*|base:rem*) continue ;;
			muldiv:mul*|muldiv:div*|muldiv:rem*) ;;
			muldiv:*) continue ;;
		esac

		"$CC" -march="$march" -mabi=ilp32 -nostdlib -nostartfiles -Wl,--no-relax \
			-T "$here/link.ld" \
			-DTEST_FUNC_NAME=mytest -DTEST_FUNC_TXT="\"$name\"" -DTEST_FUNC_RET=mytest_ret \
			-I "$tests" \
			-o "$work/$name.elf" "$here/start.S" "$src"
		"$OBJCOPY" -O binary "$work/$name.elf" "$work/$name.bin"
		od -An -tx4 -v -w4 "$work/$name.bin" | tr -d ' ' > "$out/$name.hex"
	done

	echo "built $(ls "$out"/*.hex | wc -l) $march images into $out"
}

build_set rv32i  "$fixtures/picorv32_isa"   base
build_set rv32ic "$fixtures/picorv32_isa_c" base
build_set rv32im "$fixtures/picorv32_isa_m" muldiv
