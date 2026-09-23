// Reference testbench for tests/differential_signed_compare.rs.
`timescale 1ns / 1ps

module signed_compare_test_tb;
    reg clk = 0;
    reg [7:0] a;
    reg [7:0] b;

    wire lt;
    wire gt;
    wire le;
    wire ge;
    wire unsigned_lt;

    signed_compare_test uut (
        .clk        (clk),
        .a          (a),
        .b          (b),
        .lt         (lt),
        .gt         (gt),
        .le         (le),
        .ge         (ge),
        .unsigned_lt(unsigned_lt)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d %0d", lt, gt, le, ge, unsigned_lt);
        end
    endtask

    initial begin
        // -128 vs 1: signed says less-than, unsigned (128 vs 1) says not.
        a = 8'h80;
        b = 8'h01;
        sample();

        // 1 vs -128: the same disagreement, the other way round.
        a = 8'h01;
        b = 8'h80;
        sample();

        // Equal, both positive: only the <= and >= forms are true.
        a = 8'h05;
        b = 8'h05;
        sample();

        // -1 vs -2: both negative, so ordering is the reverse of the
        // unsigned reading (255 vs 254).
        a = 8'hFF;
        b = 8'hFE;
        sample();

        // -2 vs -1.
        a = 8'hFE;
        b = 8'hFF;
        sample();

        $finish;
    end
endmodule
