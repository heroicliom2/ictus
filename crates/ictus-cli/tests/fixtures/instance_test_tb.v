// Reference testbench for tests/differential_instance.rs. Icarus simulates
// the hierarchy as written; Ictus flattens it first. The two must agree.
`timescale 1ns / 1ps

module instance_test_tb;
    reg clk = 0;
    reg resetn = 0;
    reg [7:0] a = 0, b = 0;

    wire [7:0] sum_q, diff_q, count_a, last_sum;
    wire [3:0] narrow_q;

    instance_test uut (
        .clk(clk), .resetn(resetn), .a(a), .b(b),
        .sum_q(sum_q), .diff_q(diff_q), .narrow_q(narrow_q),
        .count_a(count_a), .last_sum(last_sum)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d %0d", sum_q, diff_q, narrow_q, count_a, last_sum);
        end
    endtask

    initial begin
        // Two edges in reset, unsampled: every register is x until reset
        // reaches it in Icarus, and 0 from the start here (decisions.md D19).
        @(posedge clk); @(posedge clk); #1;
        resetn = 1;
        a = 8'd3;   b = 8'd1;   sample();
        a = 8'd10;  b = 8'd4;   sample();
        a = 8'd200; b = 8'd100; sample();
        a = 8'd7;   b = 8'd9;   sample();
        a = 8'd0;   b = 8'd0;   sample();
        $finish;
    end
endmodule
