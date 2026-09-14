// Reference testbench for tests/differential_ternary_precedence.rs.
`timescale 1ns / 1ps

module ternary_precedence_test_tb;
    reg clk = 0;
    reg [7:0] a;
    reg [7:0] c;
    wire [7:0] result;

    ternary_precedence_test uut (
        .clk   (clk),
        .a     (a),
        .c     (c),
        .result(result)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d", result);
        end
    endtask

    initial begin
        a = 8'd10;
        c = 8'd3;
        sample(); // a > c -> result should be a (10)

        a = 8'd2;
        c = 8'd8;
        sample(); // a <= c -> result should be c (8)

        a = 8'd5;
        c = 8'd5;
        sample(); // a == c (not >) -> result should be c (5)

        $finish;
    end
endmodule
