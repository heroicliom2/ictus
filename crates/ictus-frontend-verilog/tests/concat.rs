use ictus_ir::{Expr, Stmt};
use std::path::Path;

#[test]
fn lowers_concatenation_expressions() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/concat_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("concat_test.v should lower cleanly");

    let hi = module.signal_id("hi").expect("hi port");
    let lo = module.signal_id("lo").expect("lo port");
    let combined = module.signal_id("combined").expect("combined port");
    let with_flag = module.signal_id("with_flag").expect("with_flag port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };

    // combined <= {hi, lo};  -- MSB-first: hi then lo, 4+4 = 8 bits total.
    match find_assign(else_branch, combined) {
        Expr::Concat(parts) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], (Expr::Ref(id), 4) if *id == hi));
            assert!(matches!(&parts[1], (Expr::Ref(id), 4) if *id == lo));
        }
        other => panic!("expected `{{hi, lo}}`, got {other:?}"),
    }

    // with_flag <= {1'b1, hi, lo};  -- a literal part mixed with signal refs, 1+4+4 = 9 bits.
    match find_assign(else_branch, with_flag) {
        Expr::Concat(parts) => {
            assert_eq!(parts.len(), 3);
            assert!(matches!(
                &parts[0],
                (Expr::Literal { value: 1, width: 1 }, 1)
            ));
            assert!(matches!(&parts[1], (Expr::Ref(id), 4) if *id == hi));
            assert!(matches!(&parts[2], (Expr::Ref(id), 4) if *id == lo));
        }
        other => panic!("expected `{{1'b1, hi, lo}}`, got {other:?}"),
    }
}

/// A concatenation used as a *non-blocking* assignment target
/// (`{a, b} <= x;`) now lowers -- one plain `Stmt::NonBlockingAssign` per
/// part, each writing the slice of `x` that lines up with its position
/// (leftmost part = most-significant bits, same packing order a
/// concatenation *expression* uses). Before this support existed, the
/// identifier search used to find the assignment target would deep-search
/// *past* the concatenation and find `a` alone (the first identifier in
/// source order), silently discarding `b` and the split-assignment
/// semantics entirely -- this fixture (`a`, `b` both plain full-width
/// signals, no selects) is the simple case that bug would have hit.
#[test]
fn lowers_concatenation_as_nonblocking_assignment_target_with_plain_parts() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/concat_nonblocking_target_test.v");
    let module = ictus_frontend_verilog::lower_file(&path)
        .expect("concat_nonblocking_target_test.v should lower cleanly");

    let x = module.signal_id("x").expect("x port");
    let a = module.signal_id("a").expect("a port");
    let b = module.signal_id("b").expect("b port");
    let body = &module.clocked_processes[0].body;
    assert_eq!(body.len(), 2, "{{a, b}} <= x; should split into two statements");

    match &body[0] {
        Stmt::NonBlockingAssign {
            target,
            target_range: None,
            value: Expr::Select { base, msb: 3, lsb: 2 },
        } => {
            assert_eq!(*target, a);
            assert!(matches!(**base, Expr::Ref(id) if id == x));
        }
        other => panic!("expected `a <= x[3:2]` (no select on the target), got {other:?}"),
    }
    match &body[1] {
        Stmt::NonBlockingAssign {
            target,
            target_range: None,
            value: Expr::Select { base, msb: 1, lsb: 0 },
        } => {
            assert_eq!(*target, b);
            assert!(matches!(**base, Expr::Ref(id) if id == x));
        }
        other => panic!("expected `b <= x[1:0]` (no select on the target), got {other:?}"),
    }
}

/// The harder case, mirroring picorv32's own style directly: a
/// concatenation assignment target whose parts are constant
/// bit-selects/part-selects of the *same* signal
/// (`{mem_rdata_q[31:25], mem_rdata_q[11:7]} <= {...};`), mixed with a
/// plain full-width signal as another part. Confirms each part gets the
/// correct `target_range` *and* the correct slice of the (shared, cloned)
/// right-hand side.
#[test]
fn lowers_concatenation_as_nonblocking_assignment_target_with_selects() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/concat_target_select_test.v");
    let module = ictus_frontend_verilog::lower_file(&path)
        .expect("concat_target_select_test.v should lower cleanly");

    let carry = module.signal_id("carry").expect("carry port");
    let acc = module.signal_id("acc").expect("acc port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };
    assert_eq!(
        else_branch.len(),
        3,
        "{{carry, acc[7:4], acc[3:0]}} <= ...; should split into three statements"
    );

    // Every part's value is `{1'b1, data[3:0], data[7:4]}` (9 bits total),
    // sliced differently -- the same Concat expression, cloned, wrapped in
    // a different Select per part.
    let rhs_concat = |value: &Expr| match value {
        Expr::Select { base, .. } => match &**base {
            Expr::Concat(parts) => parts.len() == 3,
            _ => false,
        },
        _ => false,
    };

    match &else_branch[0] {
        Stmt::NonBlockingAssign {
            target,
            target_range: None,
            value,
        } if rhs_concat(value) => {
            assert_eq!(*target, carry);
            assert!(matches!(value, Expr::Select { msb: 8, lsb: 8, .. }));
        }
        other => panic!("expected `carry <= {{...}}[8:8]`, got {other:?}"),
    }
    match &else_branch[1] {
        Stmt::NonBlockingAssign {
            target,
            target_range: Some((7, 4)),
            value,
        } if rhs_concat(value) => {
            assert_eq!(*target, acc);
            assert!(matches!(value, Expr::Select { msb: 7, lsb: 4, .. }));
        }
        other => panic!("expected `acc[7:4] <= {{...}}[7:4]`, got {other:?}"),
    }
    match &else_branch[2] {
        Stmt::NonBlockingAssign {
            target,
            target_range: Some((3, 0)),
            value,
        } if rhs_concat(value) => {
            assert_eq!(*target, acc);
            assert!(matches!(value, Expr::Select { msb: 3, lsb: 0, .. }));
        }
        other => panic!("expected `acc[3:0] <= {{...}}[3:0]`, got {other:?}"),
    }
}

/// A concatenation used as a *continuous*-assignment target
/// (`assign {a, b} = x;`) is still rejected -- `ictus_ir::Assign` has no
/// commit phase to split a value across multiple targets in (unlike
/// `Stmt::NonBlockingAssign`, which is built fresh per part at lowering
/// time here), so only `<=` supports this.
#[test]
fn rejects_concatenation_as_continuous_assignment_target() {
    let continuous =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/concat_assign_target_test.v");
    let err = ictus_frontend_verilog::lower_file(&continuous).expect_err(
        "concat_assign_target_test.v assigns to a concatenation, which v1 must reject",
    );
    assert!(
        err.contains("concatenation"),
        "expected the error to mention concatenation, got: {err}"
    );
}

/// A *nested* concatenation inside a concatenation assignment target
/// (`{a, {b, c}} <= v;`) is rejected, not guessed at -- splitting a value
/// across a nested group needs the same recursive slicing logic all over
/// again, a real gap, not a silent-wrong-answer risk this project
/// tolerates papering over.
#[test]
fn rejects_nested_concatenation_as_assignment_target() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/concat_target_nested_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "concat_target_nested_test.v nests a concatenation inside the assignment target, which v1 must reject",
    );
    assert!(
        err.contains("nested concatenation"),
        "expected the error to mention nested concatenation, got: {err}"
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
