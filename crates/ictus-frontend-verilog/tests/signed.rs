use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// `$signed(narrow)` assigned to a wider target lowers to `Expr::Signed`,
/// carrying the operand's own natural width (6, for a `[5:0]` port) --
/// not yet extended itself; extension happens in the kernel at
/// evaluation time, using that width to locate the sign bit (see
/// `ictus_ir::Expr::Signed`'s doc comment).
#[test]
fn lowers_signed_system_function_on_a_plain_reference() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/signed_ext_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("signed_ext_test.v should lower cleanly");

    let narrow = module.signal_id("narrow").expect("narrow port");
    let wide = module.signal_id("wide").expect("wide port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };
    match &else_branch[0] {
        Stmt::NonBlockingAssign {
            target,
            target_range: None,
            value: Expr::Signed(inner, 6),
        } => {
            assert_eq!(*target, wide);
            assert!(matches!(**inner, Expr::Ref(id) if id == narrow));
        }
        other => panic!("expected `wide <= $signed(narrow)` as Expr::Signed(Ref(narrow), 6), got {other:?}"),
    }
}

/// The harder case, mirroring picorv32's own style directly:
/// `$signed({sign_bit, rest})`, a concatenation as the `$signed(...)`
/// argument -- confirms `expr_width` (already used to size concatenation
/// operands) is reused correctly to compute the *argument's* width (6:
/// 1 + 5 bits), not the assignment target's.
#[test]
fn lowers_signed_system_function_on_a_concatenation_argument() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/signed_concat_test.v");
    let module = ictus_frontend_verilog::lower_file(&path)
        .expect("signed_concat_test.v should lower cleanly");

    let wide = module.signal_id("wide").expect("wide port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };
    match &else_branch[0] {
        Stmt::NonBlockingAssign {
            target,
            target_range: None,
            value: Expr::Signed(inner, 6),
        } => {
            assert_eq!(*target, wide);
            assert!(matches!(**inner, Expr::Concat(ref parts) if parts.len() == 2));
        }
        other => panic!(
            "expected `wide <= $signed({{sign_bit, rest}})` as Expr::Signed(Concat(..), 6), got {other:?}"
        ),
    }
}

// A `$signed(...)` operand of an ordering comparison used to be rejected
// outright here; both-signed comparisons are supported now (decisions.md
// D21), and the mixed signed/unsigned case that's still rejected is
// covered by signed_compare.rs alongside the positive cases.
