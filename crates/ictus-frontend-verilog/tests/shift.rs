use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// `<<` and `>>` lower to plain logical shifts; `>>>` lowers to
/// `Expr::AShr` only when its left operand is `$signed(...)` (the one
/// case where Verilog makes it differ from `>>` at all) and to an
/// ordinary `Expr::Shr` otherwise, which is the LRM's own rule for an
/// unsigned operand rather than an approximation. Mirrors picorv32's
/// `reg_op1 << reg_op2[4:0]`, `reg_op1 >> 4`, and
/// `$signed(reg_op1) >>> 4`.
#[test]
fn lowers_shift_operators() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shift_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("shift_test.v should lower cleanly");

    let value = module.signal_id("value").expect("value port");
    let amount = module.signal_id("amount").expect("amount port");
    let shl_out = module.signal_id("shl_out").expect("shl_out port");
    let shr_out = module.signal_id("shr_out").expect("shr_out port");
    let ashr_out = module.signal_id("ashr_out").expect("ashr_out port");
    let ashr_unsigned_out = module
        .signal_id("ashr_unsigned_out")
        .expect("ashr_unsigned_out port");

    let body = &module.clocked_processes[0].body;

    match find_assign(body, shl_out) {
        Expr::Shl(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Ref(id) if id == value));
            assert!(matches!(**rhs, Expr::Ref(id) if id == amount));
        }
        other => panic!("expected `value << amount` as Expr::Shl, got {other:?}"),
    }
    match find_assign(body, shr_out) {
        Expr::Shr(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Ref(id) if id == value));
            assert!(matches!(**rhs, Expr::Ref(id) if id == amount));
        }
        other => panic!("expected `value >> amount` as Expr::Shr, got {other:?}"),
    }
    // $signed(value) >>> amount -- the arithmetic form, carrying the
    // operand's own width (8) on the Signed node so the kernel knows
    // which bit is the sign bit.
    match find_assign(body, ashr_out) {
        Expr::AShr(lhs, rhs) => {
            match &**lhs {
                Expr::Signed(inner, 8) => {
                    assert!(matches!(**inner, Expr::Ref(id) if id == value))
                }
                other => panic!("expected `$signed(value)` with width 8, got {other:?}"),
            }
            assert!(matches!(**rhs, Expr::Ref(id) if id == amount));
        }
        other => panic!("expected `$signed(value) >>> amount` as Expr::AShr, got {other:?}"),
    }
    // value >>> amount, *without* $signed -- an ordinary logical shift.
    match find_assign(body, ashr_unsigned_out) {
        Expr::Shr(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Ref(id) if id == value));
            assert!(matches!(**rhs, Expr::Ref(id) if id == amount));
        }
        other => panic!(
            "expected `value >>> amount` (unsigned operand) to lower as a logical Expr::Shr, \
             got {other:?}"
        ),
    }
}

/// A *logical* right shift of a `$signed(...)` value is rejected rather
/// than silently shifting that value's sign-extension bits down in place
/// of the zeros Verilog's `>>` actually specifies -- the same discipline
/// as the existing signed-ordering-comparison guard.
#[test]
fn rejects_logical_right_shift_of_a_signed_value() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shift_signed_logical_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "shift_signed_logical_test.v logically right-shifts a $signed value, which v1 must reject",
    );
    assert!(
        err.contains("logical right shift"),
        "expected the error to mention the logical right shift, got: {err}"
    );
}

fn find_assign(stmts: &[Stmt], target: ictus_ir::SignalId) -> &Expr {
    stmts
        .iter()
        .find_map(|s| match s {
            Stmt::NonBlockingAssign { target: t, value, .. } if *t == target => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no assignment found for signal id {target}"))
}
