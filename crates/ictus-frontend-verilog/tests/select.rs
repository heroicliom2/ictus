use ictus_ir::{Expr, Stmt};
use std::path::Path;

#[test]
fn lowers_bit_and_part_select() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/select_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("select_test.v should lower cleanly");

    let data = module.signal_id("data").expect("data port");
    let low_byte = module.signal_id("low_byte").expect("low_byte port");
    let high_byte = module.signal_id("high_byte").expect("high_byte port");
    let msb_bit = module.signal_id("msb_bit").expect("msb_bit port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };

    match find_assign(else_branch, low_byte) {
        Expr::Select { base, msb: 7, lsb: 0 } => {
            assert!(matches!(**base, Expr::Ref(id) if id == data));
        }
        other => panic!("expected `data[7:0]`, got {other:?}"),
    }

    match find_assign(else_branch, high_byte) {
        Expr::Select { base, msb: 15, lsb: 8 } => {
            assert!(matches!(**base, Expr::Ref(id) if id == data));
        }
        other => panic!("expected `data[15:8]`, got {other:?}"),
    }

    match find_assign(else_branch, msb_bit) {
        Expr::Select { base, msb: 15, lsb: 15 } => {
            assert!(matches!(**base, Expr::Ref(id) if id == data));
        }
        other => panic!("expected `data[15]` (msb == lsb == 15), got {other:?}"),
    }
}

/// A constant bit-select/part-select assignment target (`result[3:0] <=
/// v;`) lowers to a `NonBlockingAssign` carrying the write range, not a
/// (silently wrong) full-width write to `result` -- see
/// `ictus_kernel`'s read-modify-write commit logic for the other half of
/// this feature.
#[test]
fn lowers_select_as_assignment_target() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/select_target_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("select_target_test.v should lower cleanly");

    let result = module.signal_id("result").expect("result port");
    match &module.clocked_processes[0].body[0] {
        Stmt::NonBlockingAssign {
            target,
            target_range,
            value: Expr::Literal { value: 15, width: 4 },
        } => {
            assert_eq!(*target, result);
            assert_eq!(*target_range, Some((3, 0)));
        }
        other => panic!("expected `result[3:0] <= 4'hF` with a (3,0) target range, got {other:?}"),
    }
}

/// A variable-indexed bit-select target (`result[i] <= v;`) must still be
/// rejected -- the kernel's commit-phase read-modify-write needs the
/// write range known at lowering time, not computed per-cycle.
#[test]
fn rejects_variable_indexed_select_as_assignment_target() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/select_target_variable_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "select_target_variable_test.v assigns to a variable-indexed bit-select, which v1 must reject",
    );
    assert!(
        err.contains("variable-indexed"),
        "expected the error to mention the variable-indexed bit-select, got: {err}"
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
