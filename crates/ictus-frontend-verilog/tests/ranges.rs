//! Declared ranges -- `reg [8:1] r`, `reg [0:7] q`, `reg [7:0] mem [1:4]`
//! -- and the `signed` keyword. See docs/decisions.md D34.
//!
//! The frontend used to keep only a range's *width*, so every index was
//! taken as a bit position counted from 0: `r[1]` of `reg [8:1] r` read
//! the second bit instead of the first, and `q[0]` of `reg [0:7] q` read
//! the least significant bit instead of the most. Now each index is
//! translated through the declaration. These tests pin the translated
//! shapes; `ictus-cli/tests/differential_range.rs` checks the values
//! against Icarus.

use ictus_ir::{Expr, Module, Stmt};

fn lower_source(name: &str, source: &str) -> Result<Module, String> {
    let dir = std::env::temp_dir().join("ictus-frontend-range-tests");
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    let path = dir.join(format!("{name}.v"));
    std::fs::write(&path, source).expect("failed to write test source");
    ictus_frontend_verilog::lower_file(&path)
}

/// A module declaring `reg [8:1] r`, `reg [0:7] q` and `reg [7:0] mem
/// [1:4]`, with `body` in a clocked block.
fn with_ranges(body: &str) -> String {
    format!(
        "module t (input clk, input [7:0] d, input [2:0] i, output reg [7:0] y);
            reg [8:1] r;
            reg [0:7] q;
            reg [7:0] mem [1:4];
            always @(posedge clk) begin
                {body}
            end
        endmodule"
    )
}

/// The value assigned to `y` by the first statement of the clocked block.
fn value_of_y(module: &Module) -> &Expr {
    let y = module.signal_id("y").expect("y");
    module.clocked_processes[0]
        .body
        .iter()
        .find_map(|s| match s {
            Stmt::NonBlockingAssign { target, value, .. } if *target == y => Some(value),
            _ => None,
        })
        .expect("an assignment to y")
}

/// A runtime index is read whole, so the width pass cuts its arithmetic to
/// the 32 bits it is evaluated at (decisions.md D31) -- which is what turns
/// an index below the declared range into a huge, out-of-range position.
/// Returns the subtraction inside that cut.
fn index_arithmetic(index: &Expr) -> &Expr {
    match index {
        Expr::Select { base, msb: 31, lsb: 0 } => base,
        other => panic!("expected an index cut to 32 bits, got {other:?}"),
    }
}

fn assert_rejected(name: &str, source: &str, needle: &str) {
    let err = lower_source(name, source).expect_err("this must be rejected");
    assert!(
        err.contains(needle),
        "expected an error mentioning {needle:?}, got: {err}"
    );
}

/// Constant selects become bit positions: on `[8:1]` index 1 is position
/// 0; on `[0:7]` index 0 is position 7, the most significant bit.
#[test]
fn translates_constant_selects_through_the_declared_range() {
    let cases = [
        ("y <= r[1];", (0, 0)),
        ("y <= r[8];", (7, 7)),
        ("y <= r[4:1];", (3, 0)),
        ("y <= r[2 +: 4];", (4, 1)),
        ("y <= q[0];", (7, 7)),
        ("y <= q[7];", (0, 0)),
        ("y <= q[0:3];", (7, 4)),
        ("y <= q[2 +: 4];", (5, 2)),
    ];
    for (index, (body, (msb, lsb))) in cases.iter().enumerate() {
        let module = lower_source(&format!("const_{index}"), &with_ranges(body))
            .unwrap_or_else(|e| panic!("{body} should lower: {e}"));
        match value_of_y(&module) {
            Expr::Select {
                msb: got_msb,
                lsb: got_lsb,
                ..
            } => assert_eq!((*got_msb, *got_lsb), (*msb, *lsb), "{body}"),
            other => panic!("{body}: expected a select, got {other:?}"),
        }
    }
}

/// A runtime index is offset to a position at simulation time: `r[i]` on
/// `[8:1]` reads position `i - 1`, and `q[i]` on `[0:7]` position `7 - i`.
#[test]
fn offsets_a_runtime_index() {
    let module = lower_source("dyn_desc", &with_ranges("y <= r[i];")).expect("r[i] lowers");
    match value_of_y(&module) {
        Expr::DynamicBitSelect { index, .. } => assert!(
            matches!(index_arithmetic(index), Expr::Sub(_, lsb) if matches!(**lsb, Expr::Literal { value: 1, .. })),
            "expected i - 1, got {index:?}"
        ),
        other => panic!("expected a dynamic bit-select, got {other:?}"),
    }

    let module = lower_source("dyn_asc", &with_ranges("y <= q[i];")).expect("q[i] lowers");
    match value_of_y(&module) {
        Expr::DynamicBitSelect { index, .. } => assert!(
            matches!(index_arithmetic(index), Expr::Sub(right, _) if matches!(**right, Expr::Literal { value: 7, .. })),
            "expected 7 - i, got {index:?}"
        ),
        other => panic!("expected a dynamic bit-select, got {other:?}"),
    }
}

/// An array's elements are numbered as declared: `mem [1:4]` stores
/// `mem[1]` in slot 0.
#[test]
fn offsets_an_array_index_by_its_lowest_element() {
    let module = lower_source("mem_read", &with_ranges("y <= mem[i];")).expect("mem[i] lowers");
    match value_of_y(&module) {
        Expr::ArrayRead { index, .. } => assert!(
            matches!(index_arithmetic(index), Expr::Sub(_, low) if matches!(**low, Expr::Literal { value: 1, .. })),
            "expected i - 1, got {index:?}"
        ),
        other => panic!("expected an array read, got {other:?}"),
    }
}

/// Write targets are translated too: `r[4:1] <= ...` writes positions 3
/// down to 0, and `q[0:3] <= ...` the four most significant.
#[test]
fn translates_target_selects() {
    for (body, name, range) in [
        ("r[4:1] <= d[3:0];", "r", (3, 0)),
        ("q[0:3] <= d[3:0];", "q", (7, 4)),
        ("q[0] <= d[0];", "q", (7, 7)),
    ] {
        let module = lower_source("target", &with_ranges(body))
            .unwrap_or_else(|e| panic!("{body} should lower: {e}"));
        let target = module.signal_id(name).unwrap();
        let written = module.clocked_processes[0].body.iter().find_map(|s| match s {
            Stmt::NonBlockingAssign {
                target: t,
                target_range,
                ..
            } if *t == target => *target_range,
            _ => None,
        });
        assert_eq!(written, Some(range), "{body}");
    }
}

/// Verilog requires a part-select to run the same way as its
/// declaration. `[0:3]` on an ascending range is fine; `[1:4]` on a
/// descending one, and `[3:0]` on an ascending one, are errors.
#[test]
fn rejects_a_part_select_running_the_wrong_way() {
    assert_rejected("rev_desc", &with_ranges("y <= r[1:4];"), "runs the opposite way");
    assert_rejected("rev_asc", &with_ranges("y <= q[3:0];"), "runs the opposite way");
    assert_rejected("rev_target", &with_ranges("r[1:4] <= d[3:0];"), "runs the opposite way");
}

/// An index the declaration doesn't have is out of range -- `r[0]` on
/// `[8:1]` -- even though position 0 exists.
#[test]
fn rejects_a_constant_index_outside_the_declared_range() {
    assert_rejected("oob_low", &with_ranges("y <= r[0];"), "outside its declared range `[8:1]`");
    assert_rejected("oob_high", &with_ranges("y <= q[8];"), "outside its declared range `[0:7]`");
}

/// `signed` was accepted and ignored, so `s >>> 1` of a negative `s`
/// shifted in zeros. It is rejected until v1 knows which literals are
/// signed -- on a variable, a port, and a parameter alike.
#[test]
fn rejects_signed_declarations() {
    assert_rejected(
        "signed_reg",
        "module t (input clk, input [7:0] d, output reg [7:0] y);
            reg signed [7:0] s;
            always @(posedge clk) begin s <= d; y <= s >>> 1; end
        endmodule",
        "declared `signed`",
    );
    assert_rejected(
        "signed_port",
        "module t (input clk, input signed [7:0] d, output reg [7:0] y);
            always @(posedge clk) y <= d;
        endmodule",
        "port 'd' is declared `signed`",
    );
    assert_rejected(
        "signed_param",
        "module t #(parameter signed [7:0] P = 1) (input clk, output reg [7:0] y);
            always @(posedge clk) y <= P;
        endmodule",
        "declared `signed`",
    );
}

/// A parameter becomes a plain literal wherever it is used, carrying no
/// declared range, so only `[n:0]` is accepted for one.
#[test]
fn rejects_a_parameter_range_not_ending_at_zero() {
    assert_rejected(
        "param_range",
        "module t #(parameter [8:1] P = 1) (input clk, output reg [7:0] y);
            always @(posedge clk) y <= P;
        endmodule",
        "`[8:1]`",
    );
}
