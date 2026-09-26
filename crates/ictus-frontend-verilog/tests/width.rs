use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// `expr_width` answers for a binary bitwise or arithmetic result, using
/// Verilog's *self-determined* width rule: the wider of the two operands.
/// Two callers need that answer -- packing a concatenation operand into
/// its bit position, and telling a unary operator how many bits it covers
/// -- and this fixture exercises both. Mirrors picorv32's
/// `|(irq_pending & ~irq_mask)`, which is a reduction, not a
/// concatenation. See docs/decisions.md D24.
#[test]
fn records_self_determined_widths_for_bitwise_and_arithmetic_results() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/width_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("width_test.v should lower cleanly");

    let body = &module.clocked_processes[0].body;
    let value_for = |name: &str| -> &Expr {
        let target = module.signal_id(name).unwrap_or_else(|| panic!("signal {name}"));
        body.iter()
            .find_map(|s| match s {
                Stmt::NonBlockingAssign { target: t, value, .. } if *t == target => Some(value),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no assignment to {name}"))
    };

    // `|(a & ~b)`: the reduction folds exactly the AND's width, 4 bits --
    // the operand widths, not the 64-bit machine word it's evaluated in.
    match value_for("any_masked") {
        Expr::ReduceOr(inner, width) => {
            assert_eq!(*width, 4);
            assert!(matches!(**inner, Expr::And(..)), "got {inner:?}");
        }
        other => panic!("expected ReduceOr over an And, got {other:?}"),
    }

    // `{a & b, 2'b11}`: 4 + 2 = 6 bits.
    assert_eq!(concat_widths(value_for("concat_and")), vec![4, 2]);

    // `{a + b, 2'b11}`: also 4 + 2. Verilog truncates a 4-bit + 4-bit
    // sum back to 4 bits, so the carry is *not* packed -- the behaviour
    // the differential test then confirms against Icarus.
    assert_eq!(concat_widths(value_for("concat_add")), vec![4, 2]);

    // `{wide + a, 2'b11}`: mixed widths, so the wider operand wins.
    assert_eq!(concat_widths(value_for("concat_mixed")), vec![8, 2]);

    // `~(a + b)`: the complement covers the sum's own 4 bits.
    match value_for("not_sum") {
        Expr::BitwiseNot(inner, width) => {
            assert_eq!(*width, 4);
            assert!(matches!(**inner, Expr::Add(..)), "got {inner:?}");
        }
        other => panic!("expected BitwiseNot of an Add, got {other:?}"),
    }
}

/// A shift is its **left** operand's width -- a different rule from the
/// binary operators above. This used to be rejected wherever a width was
/// needed, because the rule hadn't been implemented (decisions.md D24);
/// with one set of width rules for the whole frontend (D31) it is known.
/// `{a << 1, 2'b11}` with a 4-bit `a` packs four bits of the shift, so
/// the bit shifted out of the top is lost, as Verilog specifies. The part
/// itself needs no truncation: the kernel masks each concatenation part
/// to its recorded width as it packs it.
#[test]
fn sizes_a_shift_by_its_left_operand() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/width_shift_test.v");
    let module = ictus_frontend_verilog::lower_file(&path)
        .expect("width_shift_test.v concatenates a shift result, which now has a width");

    let Stmt::NonBlockingAssign { value, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected `out <= {{a << 1, 2'b11}}`");
    };
    assert_eq!(concat_widths(value), vec![4, 2]);
    let Expr::Concat(parts) = value else {
        unreachable!()
    };
    assert!(matches!(parts[0].0, Expr::Shl(..)), "got {:?}", parts[0].0);
}

/// Where the width pass cuts arithmetic back to width, and where it
/// doesn't -- the demand logic in `width` (decisions.md D31). A cut
/// appears only where high bits are *read*: the operand of a right shift,
/// a comparison. Where only low bits are read -- an assignment, whose
/// write the kernel masks anyway -- there is none, which is what lets
/// `{cout, sum} <= a + b` deliver its carry. The differential test proves
/// the values; this pins the shape, so a regression that cut too much or
/// too little shows up as what it is.
#[test]
fn cuts_arithmetic_only_where_high_bits_are_read() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/width_context_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("width_context_test.v should lower");
    let body = &module.clocked_processes[0].body;
    let value_for = |name: &str| -> &Expr {
        let target = module.signal_id(name).unwrap_or_else(|| panic!("signal {name}"));
        body.iter()
            .find_map(|s| match s {
                Stmt::NonBlockingAssign { target: t, value, .. } if *t == target => Some(value),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no assignment to {name}"))
    };

    // `(a - b) >> 1` into 8 bits: the shift reads the difference's high
    // bits, so the difference is cut to the 8 bits it is evaluated at.
    match value_for("shifted") {
        Expr::Shr(operand, _) => assert!(matches!(evaluated_at(operand, 8), Expr::Sub(..))),
        other => panic!("expected a right shift, got {other:?}"),
    }
    // The same into 16 bits: the target widens the evaluation, so the cut
    // is at 16 -- and 3 - 5 keeps its borrow.
    match value_for("subshr16") {
        Expr::Shr(operand, _) => assert!(matches!(evaluated_at(operand, 16), Expr::Sub(..))),
        other => panic!("expected a right shift, got {other:?}"),
    }
    // A comparison reads its operands whole.
    match value_for("below_max") {
        Expr::Lt(lhs, _) => assert!(matches!(evaluated_at(lhs, 8), Expr::Sub(..))),
        other => panic!("expected a comparison, got {other:?}"),
    }
    // Written straight to a target: no cut at all.
    assert!(matches!(value_for("signed_add16"), Expr::Add(..)));
    // `{cout, sum} <= a + b` splits into two slices of one uncut sum, so
    // bit 8 -- the carry -- is still there for `cout`.
    for (name, bit) in [("cout", 8), ("sum", 0)] {
        match value_for(name) {
            Expr::Select { base, lsb, .. } => {
                assert_eq!(*lsb, bit, "{name}");
                assert!(matches!(**base, Expr::Add(..)), "{name}: the sum must be uncut");
            }
            other => panic!("expected a slice of a + b for {name}, got {other:?}"),
        }
    }
    // `~a` into 16 bits complements at 16 bits, not at `a`'s 8.
    assert!(matches!(value_for("not16"), Expr::BitwiseNot(_, 16)));
}

fn concat_widths(expr: &Expr) -> Vec<u32> {
    match expr {
        Expr::Concat(parts) => parts.iter().map(|(_, w)| *w).collect(),
        other => panic!("expected a concatenation, got {other:?}"),
    }
}

/// Arithmetic arrives wrapped in a select that cuts it to the width it is
/// evaluated at -- IEEE 1800 §11.6, decisions.md D31. Checks that width
/// and returns what is inside.
fn evaluated_at(expr: &Expr, width: u32) -> &Expr {
    match expr {
        Expr::Select { base, msb, lsb: 0 } if *msb + 1 == width => base,
        other => panic!("expected arithmetic cut to {width} bits, got {other:?}"),
    }
}

/// An operator mixing a `$signed(...)` operand with an unsigned one is
/// rejected. Verilog makes such an expression unsigned -- but an unsized
/// decimal literal counts as signed, and the IR can't tell which literals
/// were unsized, so `$signed(a) + 1` (signed) and `$signed(a) + b`
/// (unsigned) would look alike. Ictus used to accept this form and
/// sign-extend `a`, which was silently wrong. See decisions.md D31.
#[test]
fn rejects_mixed_signed_and_unsigned_operands() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed_signed_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("mixed_signed_test.v adds a $signed operand to an unsigned one");
    assert!(err.contains("mixes a `$signed(...)`"), "got: {err}");
}
