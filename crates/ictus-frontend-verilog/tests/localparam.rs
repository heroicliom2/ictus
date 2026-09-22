use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// `localparam`, resolved in the same pass and sharing the same table as
/// `parameter` (so a `localparam` can reference an earlier `parameter`),
/// with a value expression needing cross-parameter references, the
/// ternary operator, and multiplication -- mirrors picorv32's own
/// `regindex_bits`/`WITH_PCPI` directly. Also confirms a packed-range
/// bound (`reg [index_bits-1:0] wide_reg;`) and a bit-select *target*
/// index (`wide_reg[index_bits-1] <= 1;`) can both reference a
/// localparam, matching picorv32's `decoded_rd`/`decoded_rs1` and
/// `decoded_rs1[regindex_bits-1] <= 1;`.
#[test]
fn lowers_localparam_referencing_parameters_with_ternary_and_multiply() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/localparam_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("localparam_test.v should lower cleanly");

    // ENABLE_WIDE=1 (default) -> ternary picks 5; ENABLE_EXTRA=1 (default)
    // -> +1*2=2; index_bits = 5+2 = 7. wide_reg is therefore [6:0], 7 bits.
    let wide_reg = module.signal_id("wide_reg").expect("wide_reg internal signal");
    assert_eq!(
        module.signals[wide_reg].width, 7,
        "reg [index_bits-1:0] should resolve to a 7-bit signal"
    );

    let index_bits_out = module.signal_id("index_bits_out").expect("index_bits_out port");
    let with_feature_out = module
        .signal_id("with_feature_out")
        .expect("with_feature_out port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };

    // index_bits_out <= index_bits;  -- index_bits itself resolved to a
    // plain literal (7) at lowering time, substituted directly.
    match find_assign(else_branch, index_bits_out) {
        Expr::Literal { value: 7, .. } => {}
        other => panic!("expected `index_bits_out <= 7` (index_bits resolved), got {other:?}"),
    }

    // with_feature_out <= WITH_FEATURE;  -- 1||1 folded to 1.
    match find_assign(else_branch, with_feature_out) {
        Expr::Literal { value: 1, .. } => {}
        other => panic!("expected `with_feature_out <= 1` (WITH_FEATURE resolved), got {other:?}"),
    }

    // wide_reg[index_bits-1] <= 1'b1;  -- index_bits-1 = 6, folded to a
    // constant target_range, not rejected as variable-indexed.
    let wide_reg_assign = else_branch
        .iter()
        .find_map(|s| match s {
            Stmt::NonBlockingAssign { target: t, target_range, .. } if *t == wide_reg => {
                Some(*target_range)
            }
            _ => None,
        })
        .expect("no assignment to wide_reg found");
    assert_eq!(
        wide_reg_assign,
        Some((6, 6)),
        "wide_reg[index_bits-1] should fold to a constant target_range (6, 6)"
    );
}

/// A `localparam` whose value is a concatenation (mirroring picorv32's
/// `localparam [35:0] TRACE_BRANCH = {4'b 0001, 32'b 0};`) folds to a
/// single packed literal.
#[test]
fn lowers_localparam_with_concatenation_value() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/localparam_concat_test.v");
    let module = ictus_frontend_verilog::lower_file(&path)
        .expect("localparam_concat_test.v should lower cleanly");

    let trace_out = module.signal_id("trace_out").expect("trace_out port");
    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };
    match find_assign(else_branch, trace_out) {
        // {4'b0001, 4'b0010} = 8'b0001_0010 = 0x12 = 18.
        Expr::Literal { value: 18, width: 8 } => {}
        other => panic!("expected `trace_out <= 8'h12` (TRACE_X resolved), got {other:?}"),
    }
}

/// A constant expression (a `localparam` value, here) referencing an
/// undeclared parameter is rejected, not silently treated as 0 or
/// skipped.
#[test]
fn rejects_constant_expression_referencing_unknown_parameter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/localparam_unknown_ref_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "localparam_unknown_ref_test.v references an undeclared parameter, which v1 must reject",
    );
    assert!(
        err.contains("unknown parameter"),
        "expected the error to mention the unknown parameter, got: {err}"
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
