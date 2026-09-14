use ictus_ir::{Expr, Stmt};
use std::path::Path;

#[test]
fn lowers_variable_bit_select() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dynsel_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("dynsel_test.v should lower cleanly");

    let data = module.signal_id("data").expect("data port");
    let idx = module.signal_id("idx").expect("idx port");
    let bit_out = module.signal_id("bit_out").expect("bit_out port");

    let Stmt::NonBlockingAssign { target, value } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to be a single non-blocking assignment");
    };
    assert_eq!(*target, bit_out);

    match value {
        Expr::DynamicBitSelect { base, index } => {
            assert!(matches!(**base, Expr::Ref(id) if id == data));
            assert!(matches!(**index, Expr::Ref(id) if id == idx));
        }
        other => panic!("expected `data[idx]` to lower to DynamicBitSelect, got {other:?}"),
    }
}
