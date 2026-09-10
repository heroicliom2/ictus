// Reference testbench for tests/differential_ops.rs. Drives the same
// reset/stimulus sequence the Rust-side test drives through
// ictus_kernel::Simulation directly, and prints one space-separated line
// of every output (plus the internal `scratch` register, read via a
// hierarchical reference into the instance) per clock cycle, so the two
// traces can be diffed exactly.
`timescale 1ns / 1ps

module ops_test_tb;
    reg clk = 0;
    reg resetn;
    reg [7:0] a;
    reg [7:0] b;

    wire [7:0] and_result, or_result, xor_result;
    wire eq_flag, ne_flag, lt_flag, le_flag, gt_flag, ge_flag, logic_flag;

    ops_test uut (
        .clk        (clk),
        .resetn     (resetn),
        .a          (a),
        .b          (b),
        .and_result (and_result),
        .or_result  (or_result),
        .xor_result (xor_result),
        .eq_flag    (eq_flag),
        .ne_flag    (ne_flag),
        .lt_flag    (lt_flag),
        .le_flag    (le_flag),
        .gt_flag    (gt_flag),
        .ge_flag    (ge_flag),
        .logic_flag (logic_flag)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d",
                and_result, or_result, xor_result,
                eq_flag, ne_flag, lt_flag, le_flag, gt_flag, ge_flag,
                logic_flag, uut.scratch);
        end
    endtask

    initial begin
        resetn = 0;
        a = 8'h00;
        b = 8'h00;

        sample(); // cycle 1: held in reset
        sample(); // cycle 2: held in reset

        resetn = 1;
        a = 8'd5;
        b = 8'd5;
        sample(); // cycle 3: a == b

        a = 8'd3;
        b = 8'd9;
        sample(); // cycle 4: a < b

        a = 8'd9;
        b = 8'd3;
        sample(); // cycle 5: a > b

        a = 8'hFF;
        b = 8'h0F;
        sample(); // cycle 6: real bit patterns for bitwise ops

        $finish;
    end
endmodule
