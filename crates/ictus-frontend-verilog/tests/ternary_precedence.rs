use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// `a > c ? a : c` (no parens around the comparison) is real, common
/// Verilog style -- and unambiguous: every binary operator this frontend
/// supports binds tighter than `?:`, so this can only mean `(a > c) ? a
/// : c`. sv-parser structures it the other way, though: as `a > (c ? a :
/// c)` (a Binary node whose right operand is a bare ConditionalExpression),
/// which -- if lowered literally -- would silently compute the wrong
/// value. `lower_expr`'s `E::Binary` arm specifically detects and
/// corrects this. This test's real job is confirming the *un*parenthesized
/// form lowers identically to the parenthesized one, not just that
/// either individually "looks plausible".
#[test]
fn ternary_after_comparison_gets_correct_precedence_without_parens() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ternary_precedence_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("ternary_precedence_test.v should lower cleanly");

    let a = module.signal_id("a").expect("a port");
    let c = module.signal_id("c").expect("c port");

    let Stmt::NonBlockingAssign { value, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected a single non-blocking assignment");
    };

    let Expr::Ternary { cond, then_val, else_val } = value else {
        panic!("expected `a > c ? a : c` to lower to a top-level Ternary, got {value:?}");
    };
    assert!(
        matches!(**cond, Expr::Gt(ref l, ref r) if matches!(**l, Expr::Ref(id) if id == a) && matches!(**r, Expr::Ref(id) if id == c)),
        "expected the ternary's condition to be `a > c`, got {cond:?}"
    );
    assert!(matches!(**then_val, Expr::Ref(id) if id == a));
    assert!(matches!(**else_val, Expr::Ref(id) if id == c));
}
