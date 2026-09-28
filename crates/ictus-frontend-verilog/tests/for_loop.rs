//! `for` loops, unrolled while lowering (docs/decisions.md D33), and the
//! indexed part-selects that come with them. Values are checked against
//! Icarus in ictus-cli's `differential_for_loop.rs`; this file pins down
//! the unrolled shape, and that every restriction `lower_for` documents
//! is a rejection with its reason rather than a guess.

use ictus_ir::{Expr, Module, Stmt};

/// Lowers `source`, written to a temporary file named after the test --
/// most cases here are a few lines, clearer beside their assertion than in
/// a fixture file of their own.
fn lower_source(name: &str, source: &str) -> Result<Module, String> {
    let dir = std::env::temp_dir().join("ictus-frontend-for-loop-tests");
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    let path = dir.join(format!("{name}.v"));
    std::fs::write(&path, source).expect("failed to write test source");
    ictus_frontend_verilog::lower_file(&path)
}

/// A combinational block around `body`, with `integer i, k;` declared.
fn in_comb_block(body: &str) -> String {
    format!(
        "module t (input [7:0] a, input [7:0] b, output reg [7:0] y);
            integer i, k;
            always @* begin
                y = 0;
                {body}
            end
        endmodule"
    )
}

fn assert_rejected(name: &str, source: &str, needle: &str) {
    let err = lower_source(name, source).expect_err("this loop must be rejected");
    assert!(
        err.contains(needle),
        "expected an error mentioning {needle:?}, got: {err}"
    );
}

/// Each iteration becomes its own statement, with the loop variable's
/// value for that iteration folded into every select -- `a[i*2 +: 2]` is
/// the constant part-select `a[1:0]`, then `a[3:2]`, and so on -- and the
/// loop variable itself is not a signal at all.
#[test]
fn unrolls_into_one_copy_of_the_body_per_iteration() {
    let module = lower_source(
        "unroll",
        &in_comb_block("for (i = 0; i < 4; i = i + 1) y[i*2 +: 2] = a[i*2 +: 2];"),
    )
    .expect("a constant-bounded loop should lower");

    assert_eq!(
        module.signal_id("i"),
        None,
        "an integer loop variable is not a signal"
    );
    let a = module.signal_id("a").expect("a port");
    let y = module.signal_id("y").expect("y port");

    let body = &module.comb_processes[0].body;
    // `y = 0;`, then four copies.
    assert_eq!(
        body.len(),
        5,
        "expected `y = 0;` and four unrolled copies, got {body:?}"
    );
    for (n, stmt) in body[1..].iter().enumerate() {
        let (msb, lsb) = (n as u32 * 2 + 1, n as u32 * 2);
        match stmt {
            Stmt::BlockingAssign {
                target,
                target_range,
                value:
                    Expr::Select {
                        base,
                        msb: m,
                        lsb: l,
                    },
            } => {
                assert_eq!((*target, *target_range), (y, Some((msb, lsb))));
                assert!(matches!(**base, Expr::Ref(id) if id == a));
                assert_eq!((*m, *l), (msb, lsb));
            }
            other => {
                panic!("iteration {n}: expected y[{msb}:{lsb}] = a[{msb}:{lsb}], got {other:?}")
            }
        }
    }
}

/// `-:` selects downward from its base.
#[test]
fn indexed_part_select_downward() {
    let module = lower_source("downward", &in_comb_block("y[7 -: 4] = a[3 -: 4];"))
        .expect("a constant `-:` select should lower");
    match &module.comb_processes[0].body[1] {
        Stmt::BlockingAssign {
            target_range,
            value: Expr::Select { msb, lsb, .. },
            ..
        } => {
            assert_eq!(*target_range, Some((7, 4)));
            assert_eq!((*msb, *lsb), (3, 0));
        }
        other => panic!("expected y[7:4] = a[3:0], got {other:?}"),
    }
}

// --- Loop control ------------------------------------------------------

#[test]
fn rejects_a_loop_variable_that_is_not_an_integer() {
    assert_rejected(
        "reg_variable",
        "module t (input [7:0] a, output reg [7:0] y);
            reg [3:0] n;
            always @* begin
                y = 0;
                for (n = 0; n < 4; n = n + 1) y[n] = a[n];
            end
        endmodule",
        "isn't declared `integer`",
    );
}

#[test]
fn rejects_a_condition_that_reads_a_signal() {
    assert_rejected(
        "signal_condition",
        &in_comb_block("for (i = 0; i < b; i = i + 1) y = y + 1;"),
        "reads a signal",
    );
}

#[test]
fn rejects_increment_step() {
    assert_rejected(
        "increment_step",
        &in_comb_block("for (i = 0; i < 4; i++) y[i] = a[i];"),
        "must be written `i = ...`",
    );
}

#[test]
fn rejects_compound_step() {
    assert_rejected(
        "compound_step",
        &in_comb_block("for (i = 0; i < 4; i += 1) y[i] = a[i];"),
        "must be written `i = ...`",
    );
}

#[test]
fn rejects_a_loop_local_declaration() {
    assert_rejected(
        "local_declaration",
        &in_comb_block("for (int n = 0; n < 4; n = n + 1) y[n] = a[n];"),
        "loop-local declaration",
    );
}

#[test]
fn rejects_other_loop_kinds() {
    assert_rejected(
        "while_loop",
        &in_comb_block("while (y < 4) y = y + 1;"),
        "only `for` loops",
    );
}

/// A loop whose step never moves it towards the end is stopped, not
/// lowered forever.
#[test]
fn rejects_a_loop_that_never_ends() {
    assert_rejected(
        "never_ends",
        &in_comb_block("for (i = 0; i < 4; i = i) y = y + 1;"),
        "runs more than",
    );
}

#[test]
fn rejects_nested_loops_sharing_a_variable() {
    assert_rejected(
        "shared_variable",
        &in_comb_block("for (i = 0; i < 2; i = i + 1) for (i = 0; i < 2; i = i + 1) y = y + 1;"),
        "enclosing `for` loop",
    );
}

// --- Signedness: Verilog's `integer` is signed, v1's constants aren't ---

/// The classic descending loop runs its variable to -1 to stop. Verilog's
/// signed `integer` makes `i >= 0` false there; v1 would see a huge
/// unsigned number, for which it is true. Rejected at the step.
#[test]
fn rejects_a_loop_variable_going_negative() {
    assert_rejected(
        "goes_negative",
        &in_comb_block("for (i = 3; i >= 0; i = i - 1) y[i] = a[i];"),
        "0 - 1 in its step",
    );
}

/// `i - 1 < 0` is true in Verilog on the iteration where `i` is 0, and
/// false if evaluated unsigned. Rejected rather than guessed.
#[test]
fn rejects_negative_constant_arithmetic_in_the_body() {
    assert_rejected(
        "negative_in_body",
        &in_comb_block("for (i = 0; i < 4; i = i + 1) if (i - 1 < 0) y = y + 1;"),
        "in its body (where i = 0)",
    );
}

// --- The loop variable outside its role ---------------------------------

#[test]
fn rejects_an_integer_read_outside_a_loop() {
    assert_rejected(
        "read_outside",
        &in_comb_block("y = i;"),
        "only as a `for` loop variable",
    );
}

#[test]
fn rejects_the_body_assigning_its_own_variable() {
    assert_rejected(
        "body_assigns_variable",
        &in_comb_block("for (i = 0; i < 4; i = i + 1) i = 5;"),
        "assigns its own variable 'i'",
    );
}

/// `int` used to lower, silently, as a 1-bit signal, as `integer` did.
#[test]
fn rejects_other_integer_types() {
    assert_rejected(
        "int_variable",
        "module t (input [7:0] a, output reg [7:0] y);
            int n;
            always @* y = a;
        endmodule",
        "`int`",
    );
}

// --- Selects the loop makes easy to get wrong --------------------------

/// `a[i + 1]` on the last iteration reads past `a`'s top bit, which
/// Verilog reads as `x`.
#[test]
fn rejects_a_constant_read_past_the_top_bit() {
    assert_rejected(
        "read_past_top",
        &in_comb_block("for (i = 0; i < 8; i = i + 1) y[i] = a[i + 1];"),
        "bit 8 is out of range for a 8-bit value",
    );
}

#[test]
fn rejects_an_indexed_part_select_with_a_variable_base() {
    assert_rejected(
        "variable_base",
        &in_comb_block("y[3:0] = a[b +: 4];"),
        "base isn't a compile-time constant",
    );
}

#[test]
fn rejects_an_indexed_part_select_below_bit_zero() {
    assert_rejected(
        "below_zero",
        &in_comb_block("y[3:0] = a[1 -: 4];"),
        "reaches below bit 0",
    );
}
