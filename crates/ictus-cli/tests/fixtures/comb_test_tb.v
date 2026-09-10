// Reference testbench for tests/differential_comb.rs. Same drive/sample
// discipline as ops_test_tb.v: inputs change with margin before each
// edge, sampled `#1` after it.
`timescale 1ns / 1ps

module comb_test_tb;
    reg clk = 0;
    reg resetn;
    reg [7:0] a;
    reg [7:0] b;

    wire [7:0] sum_comb;
    wire [7:0] sum_reg;
    wire sum_high;

    comb_test uut (
        .clk     (clk),
        .resetn  (resetn),
        .a       (a),
        .b       (b),
        .sum_comb(sum_comb),
        .sum_reg (sum_reg),
        .sum_high(sum_high)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d", sum_comb, sum_reg, sum_high);
        end
    endtask

    initial begin
        resetn = 0;
        a = 8'h00;
        b = 8'h00;

        sample(); // cycle 1: held in reset
        sample(); // cycle 2: held in reset

        resetn = 1;

        a = 100;
        b = 50;
        sample(); // cycle 3: sum_comb=150 settles pre-edge -> sum_reg becomes 150

        a = 10;
        b = 20;
        sample(); // cycle 4: sum_reg becomes 30 (< 128 -> sum_high false)

        a = 200;
        b = 100;
        sample(); // cycle 5: sum_comb = 300 truncated to 8 bits = 44

        a = 128;
        b = 0;
        sample(); // cycle 6: sum_reg becomes 128 -> sum_high true (boundary)

        $finish;
    end
endmodule
