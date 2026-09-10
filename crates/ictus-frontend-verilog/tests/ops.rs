use ictus_ir::{Expr, Stmt};
use std::path::Path;

#[test]
fn lowers_operators_literals_and_internal_signals() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ops_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("ops_test.v should lower cleanly");

    // Internal signal (no port direction), declared in the module body.
    let scratch = module.signal_id("scratch").expect("scratch signal");
    assert_eq!(module.signals[scratch].direction, None);
    assert_eq!(module.signals[scratch].width, 8);

    assert_eq!(module.clocked_processes.len(), 1);
    let Stmt::If {
        then_branch,
        else_branch,
        ..
    } = &module.clocked_processes[0].body[0]
    else {
        panic!("expected the process body to be a single if/else statement");
    };

    // Reset branch: hex literals with declared widths.
    let and_result = module.signal_id("and_result").unwrap();
    match find_assign(then_branch, and_result) {
        Expr::Literal { value: 0, width: 8 } => {}
        other => panic!("expected `8'h00`, got {other:?}"),
    }
    let scratch_reset = find_assign(then_branch, scratch);
    match scratch_reset {
        Expr::Literal { value: 0xFF, width: 8 } => {}
        other => panic!("expected `8'hFF`, got {other:?}"),
    }
    let eq_flag = module.signal_id("eq_flag").unwrap();
    match find_assign(then_branch, eq_flag) {
        Expr::Literal { value: 0, width: 1 } => {}
        other => panic!("expected `1'b0`, got {other:?}"),
    }

    // Active branch: every new binary operator lowers to its own Expr
    // variant, and the binary literal with an underscore separator
    // (`8'b0000_1111`) parses to the right value/width.
    assert!(matches!(find_assign(else_branch, and_result), Expr::And(..)));
    let or_result = module.signal_id("or_result").unwrap();
    assert!(matches!(find_assign(else_branch, or_result), Expr::Or(..)));
    let xor_result = module.signal_id("xor_result").unwrap();
    assert!(matches!(find_assign(else_branch, xor_result), Expr::Xor(..)));
    assert!(matches!(find_assign(else_branch, eq_flag), Expr::Eq(..)));
    let ne_flag = module.signal_id("ne_flag").unwrap();
    assert!(matches!(find_assign(else_branch, ne_flag), Expr::Ne(..)));
    let lt_flag = module.signal_id("lt_flag").unwrap();
    assert!(matches!(find_assign(else_branch, lt_flag), Expr::Lt(..)));
    let le_flag = module.signal_id("le_flag").unwrap();
    assert!(matches!(find_assign(else_branch, le_flag), Expr::Le(..)));
    let gt_flag = module.signal_id("gt_flag").unwrap();
    assert!(matches!(find_assign(else_branch, gt_flag), Expr::Gt(..)));
    let ge_flag = module.signal_id("ge_flag").unwrap();
    assert!(matches!(find_assign(else_branch, ge_flag), Expr::Ge(..)));
    let logic_flag = module.signal_id("logic_flag").unwrap();
    match find_assign(else_branch, logic_flag) {
        Expr::LogicalAnd(lhs, _) => {
            assert!(matches!(**lhs, Expr::LogicalOr(..)), "expected (eq||ne) && resetn");
        }
        other => panic!("expected `(eq_flag || ne_flag) && resetn`, got {other:?}"),
    }

    match find_assign(else_branch, scratch) {
        Expr::And(_, rhs) => match **rhs {
            Expr::Literal { value: 0x0F, width: 8 } => {}
            ref other => panic!("expected `8'b0000_1111` (0x0F), got {other:?}"),
        },
        other => panic!("expected `scratch & 8'b0000_1111`, got {other:?}"),
    }
}

fn find_assign(stmts: &[Stmt], target: ictus_ir::SignalId) -> &Expr {
    stmts
        .iter()
        .find_map(|s| match s {
            Stmt::NonBlockingAssign { target: t, value } if *t == target => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no assignment found for signal id {target}"))
}
