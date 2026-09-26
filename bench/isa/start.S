# Minimal entry point for running one picorv32 per-instruction test on its
# own, in picorv32's *default* configuration (base RV32I only).
#
# picorv32's own firmware calls each test as a function and chains them;
# each test prints its name followed by "OK" or "ERROR" by storing bytes
# to 0x10000000, and jumps to <name>_ret when it passes. This stub jumps
# straight into the one test being built and, when it returns, executes
# `ebreak` -- which traps the core. A failing test also ends in `ebreak`,
# so *every* run ends with picorv32's `trap` output high, and which of the
# two it was is visible in the characters it printed.

	.section .text.start
	.global _start
_start:
	jal	zero, mytest

	.global mytest_ret
mytest_ret:
	ebreak
