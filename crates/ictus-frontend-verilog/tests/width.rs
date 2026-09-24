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

/// A *shift* result still has no width here. Verilog gives `a << b` its
/// **left** operand's width -- a different rule from the binary operators
/// above, not the same change twice -- so it is rejected with an error
/// naming that rather than being folded in on the assumption the rules
/// match.
#[test]
fn rejects_a_shift_where_a_width_is_needed() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/width_shift_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("width_shift_test.v concatenates a shift result, which v1 must reject");
    assert!(
        err.contains("shift"),
        "expected the error to name the shift rule, got: {err}"
    );
}

fn concat_widths(expr: &Expr) -> Vec<u32> {
    match expr {
        Expr::Concat(parts) => parts.iter().map(|(_, w)| *w).collect(),
        other => panic!("expected a concatenation, got {other:?}"),
    }
}
