// Reference testbench for tests/differential_width_context.rs.
`timescale 1ns / 1ps

module width_context_test_tb;
    reg clk = 0;
    reg [7:0] a, b;
    wire [7:0] shifted, neg_shifted;
    wire below_max, wrapped_eq;

    width_context_test uut (
        .clk(clk), .a(a), .b(b), .shifted(shifted), .below_max(below_max),
        .neg_shifted(neg_shifted), .wrapped_eq(wrapped_eq)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d", shifted, below_max, neg_shifted, wrapped_eq);
        end
    endtask

    initial begin
        a = 8'd3;  b = 8'd5;  sample();   // wraps: 3 - 5 = 254 in 8 bits
        a = 8'd9;  b = 8'd4;  sample();   // does not wrap
        a = 8'd0;  b = 8'd1;  sample();   // wraps to 255
        $finish;
    end
endmodule
