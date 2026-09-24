use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// `sv-parser` returns a binary expression as a right-leaning chain in
/// source order with no precedence applied, so `a == b && c == d` arrives
/// as `a == (b && (c == d))`. Lowering that literally computes a
/// different value than Verilog specifies. `lower_binary_chain`
/// re-associates it; this test pins the resulting shape, since the value
/// alone can agree by coincidence on a given input (it did, on the first
/// vectors tried by hand). See docs/decisions.md D25.
#[test]
fn reassociates_binary_chains_by_verilog_precedence() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/precedence_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("precedence_test.v should lower cleanly");

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

    // `==` binds tighter than `&&`: the AND is on top, with a comparison
    // under each arm -- not a comparison against the result of an AND.
    match value_for("decoded") {
        Expr::LogicalAnd(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Eq(..)), "left arm: {lhs:?}");
            assert!(matches!(**rhs, Expr::Eq(..)), "right arm: {rhs:?}");
        }
        other => panic!("expected `(d[14:12] == 1) && (d[31:25] == 0)`, got {other:?}"),
    }

    // Left-associative: `a - b - c` is `(a - b) - c`. The parser's own
    // nesting says `a - (b - c)`, which differs by 2*c.
    match value_for("left_assoc") {
        Expr::Sub(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Sub(..)), "expected the nesting on the left: {lhs:?}");
            assert!(matches!(**rhs, Expr::Ref(_)), "expected a bare operand: {rhs:?}");
        }
        other => panic!("expected `(a - b) - c`, got {other:?}"),
    }

    // `*` binds tighter than `+`, so here the parser's own right-leaning
    // nesting is already correct and must be left alone.
    match value_for("mixed_arith") {
        Expr::Add(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Ref(_)), "expected a bare operand: {lhs:?}");
            assert!(matches!(**rhs, Expr::Mul(..)), "expected the product on the right: {rhs:?}");
        }
        other => panic!("expected `a + (b * c)`, got {other:?}"),
    }

    // A bare ternary binds looser than every binary operator, so it can
    // only end a chain and everything to its left is really its
    // condition. This is the case decisions.md D13 originally fixed on
    // its own; it now falls out of the general rule.
    match value_for("tern") {
        Expr::Ternary { cond, .. } => {
            assert!(matches!(**cond, Expr::Gt(..)), "expected `a > b` as the condition: {cond:?}");
        }
        other => panic!("expected a ternary with a comparison condition, got {other:?}"),
    }
}
