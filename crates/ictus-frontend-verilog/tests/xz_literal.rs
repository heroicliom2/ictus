use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// A 4-state `x`/`z` digit outside a `case` item -- whole-value (`8'bx`,
/// `8'hxx`, `8'dx`) or mixed with real digits (`4'b10x1`), and as a
/// concatenation operand (`{4'bx, data}`) -- resolves to the bit `0`
/// (decisions.md D19), matching Verilator's own default X-handling
/// policy. Mirrors picorv32's own `assign pcpi_mul_rd = 32'bx;`,
/// `decoded_imm <= 1'bx;`, and `{16'bx, mem_16bit_buffer}` styles.
#[test]
fn lowers_xz_literals_as_zero() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xz_literal_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("xz_literal_test.v should lower cleanly");

    let data = module.signal_id("data").expect("data port");
    let bin_out = module.signal_id("bin_out").expect("bin_out port");
    let hex_out = module.signal_id("hex_out").expect("hex_out port");
    let dec_out = module.signal_id("dec_out").expect("dec_out port");
    let mixed_out = module.signal_id("mixed_out").expect("mixed_out port");
    let concat_out = module.signal_id("concat_out").expect("concat_out port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };

    match find_assign(else_branch, bin_out) {
        Expr::Literal { value: 0, width: 8 } => {}
        other => panic!("expected `bin_out <= 8'h00` (8'bx resolved to 0), got {other:?}"),
    }
    match find_assign(else_branch, hex_out) {
        Expr::Literal { value: 0, width: 8 } => {}
        other => panic!("expected `hex_out <= 8'h00` (8'hxx resolved to 0), got {other:?}"),
    }
    match find_assign(else_branch, dec_out) {
        Expr::Literal { value: 0, width: 8 } => {}
        other => panic!("expected `dec_out <= 8'h00` (8'dx resolved to 0), got {other:?}"),
    }
    // 4'b10x1 -- the 'x' digit (bit 1) resolves to 0, giving 0b1001 = 9.
    match find_assign(else_branch, mixed_out) {
        Expr::Literal { value: 9, width: 4 } => {}
        other => panic!("expected `mixed_out <= 4'b1001` (4'b10x1 with x->0), got {other:?}"),
    }
    // {4'bx, data} -- the x-part resolves to a 4-bit 0, concatenated
    // ahead of data.
    match find_assign(else_branch, concat_out) {
        Expr::Concat(parts) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], (Expr::Literal { value: 0, .. }, 4)));
            assert!(matches!(&parts[1], (Expr::Ref(id), 4) if *id == data));
        }
        other => panic!("expected `{{4'bx, data}}` as Expr::Concat, got {other:?}"),
    }
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
