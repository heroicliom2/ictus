use ictus_ir::Expr;
use std::path::Path;

/// A net declaration carrying an initializer (`wire doubled = a + a;`) is
/// a continuous assignment -- IEEE 1800 defines it as exactly equivalent
/// to `wire doubled; assign doubled = a + a;`. picorv32 drives most of
/// its combinational logic this way (59 such declarations against 43
/// standalone `assign` statements), and these used to be dropped
/// silently: the wire existed, read 0 forever, and the design still
/// lowered and ran. See docs/decisions.md D25.
#[test]
fn lowers_net_declaration_initializers_as_continuous_assignments() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/net_init_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("net_init_test.v should lower cleanly");

    let doubled = module.signal_id("doubled").expect("doubled wire");
    let a_big = module.signal_id("a_big").expect("a_big wire");
    let sum = module.signal_id("sum").expect("sum port");
    let both = module.signal_id("both").expect("both port");

    // Both forms produce the same thing, and all four are present: the
    // two standalone `assign`s and the two declaration initializers.
    assert_eq!(module.assigns.len(), 4, "got {:?}", module.assigns);
    let target_of = |id| {
        module
            .assigns
            .iter()
            .find(|assign| assign.target == id)
            .unwrap_or_else(|| panic!("no continuous assignment drives signal {id}"))
    };
    assert!(matches!(target_of(doubled).value, Expr::Add(..)));
    assert!(matches!(target_of(a_big).value, Expr::Gt(..)));
    assert!(matches!(target_of(sum).value, Expr::Add(..)));
    assert!(matches!(target_of(both).value, Expr::LogicalAnd(..)));
}

/// A *variable* initializer (`reg [7:0] count = 8'd3;`) looks like the
/// net form and means something else entirely: it runs once before time
/// zero and the variable changes freely afterwards, where a net
/// initializer drives the net for the whole simulation. v1 has no
/// initialization phase, so it is rejected rather than either mis-lowered
/// into a continuous assignment (which would pin the register forever) or
/// quietly ignored (right only for `= 0`).
#[test]
fn rejects_variable_initializer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reg_initializer_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("reg_initializer_test.v initializes a reg, which v1 must reject");
    assert!(
        err.contains("initializer"),
        "expected the error to name the initializer, got: {err}"
    );
}
