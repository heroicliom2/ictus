use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// An array declaration (`reg [7:0] mem [0:3];` -- Verilog's *unpacked*
/// dimension) gives the signal a `depth`, and an index on it selects an
/// *element* rather than a bit, on both the read and the write side.
/// Mirrors picorv32's own register file: `reg [31:0] cpuregs
/// [0:regfile_size-1];`, written as `cpuregs[latched_rd] <= ...`.
#[test]
fn lowers_array_declaration_reads_and_writes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/array_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("array_test.v should lower cleanly");

    let mem = module.signal_id("mem").expect("mem array signal");
    let raddr = module.signal_id("raddr").expect("raddr port");
    let waddr = module.signal_id("waddr").expect("waddr port");
    let wdata = module.signal_id("wdata").expect("wdata port");
    let rdata = module.signal_id("rdata").expect("rdata port");
    let fixed_read = module.signal_id("fixed_read").expect("fixed_read port");

    // The declaration: 4 elements of 8 bits each -- `width` stays the
    // *element* width, with the unpacked dimension carried separately.
    assert_eq!(module.signals[mem].width, 8);
    assert_eq!(module.signals[mem].depth, Some(4));
    // A scalar keeps `depth: None`.
    assert_eq!(module.signals[rdata].depth, None);

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };

    // The write is its own statement kind, with the index kept as an
    // expression to evaluate against the pre-edge snapshot.
    let Stmt::If { then_branch, .. } = &else_branch[0] else {
        panic!("expected the write to sit inside `if (write_enable)`");
    };
    match &then_branch[0] {
        Stmt::ArrayAssign { array, index, value } => {
            assert_eq!(*array, mem);
            assert!(matches!(index, Expr::Ref(id) if *id == waddr));
            assert!(matches!(value, Expr::Ref(id) if *id == wdata));
        }
        other => panic!("expected `mem[waddr] <= wdata` as Stmt::ArrayAssign, got {other:?}"),
    }

    // A runtime-indexed read.
    match find_assign(else_branch, rdata) {
        Expr::ArrayRead { array, index } => {
            assert_eq!(*array, mem);
            assert!(matches!(**index, Expr::Ref(id) if id == raddr));
        }
        other => panic!("expected `mem[raddr]` as Expr::ArrayRead, got {other:?}"),
    }
    // A *constant* index is still an element read, not a bit-select --
    // the array check has to come first, or `mem[2]` would read bit 2.
    match find_assign(else_branch, fixed_read) {
        Expr::ArrayRead { array, index } => {
            assert_eq!(*array, mem);
            assert!(matches!(**index, Expr::Literal { value: 2, .. }));
        }
        other => panic!("expected `mem[2]` as Expr::ArrayRead, got {other:?}"),
    }
}

/// An array as a module *port* is rejected -- it would have to be
/// addressable from outside the module, which nothing in v1 (no
/// instantiation) can do.
#[test]
fn rejects_array_port() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/array_port_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("array_port_test.v declares an array port, which v1 must reject");
    assert!(
        err.contains("array"),
        "expected the error to mention the array port, got: {err}"
    );
}

/// A bit-select *of an array element* (`mem[i][3]`) is rejected rather
/// than silently dropping the bit index: `Stmt::ArrayAssign` can't
/// represent a partial element write, and allowing it on reads alone
/// would be a confusing asymmetry. (It reaches the frontend as a second
/// index bracket, structurally indistinguishable from a genuinely
/// multi-dimensional array -- neither is supported, and the error says
/// so.)
#[test]
fn rejects_bit_select_of_an_array_element() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/array_element_bitselect_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "array_element_bitselect_test.v bit-selects an array element, which v1 must reject",
    );
    assert!(
        err.contains("indexing array element"),
        "expected the error to mention indexing the element further, got: {err}"
    );
}

/// The unpacked dimension is found by searching the whole declaration,
/// which can't tell which declared name it belongs to -- so a declaration
/// mixing an array with other names is rejected rather than wrongly
/// making all of them arrays.
#[test]
fn rejects_array_declared_alongside_other_names() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/array_multiname_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "array_multiname_test.v declares an array alongside a scalar, which v1 must reject",
    );
    assert!(
        err.contains("declare the array on its own"),
        "expected the error to say to split the declaration, got: {err}"
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
