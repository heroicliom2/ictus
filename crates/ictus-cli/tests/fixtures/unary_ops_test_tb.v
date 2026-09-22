// Reference testbench for tests/differential_unary_ops.rs.
`timescale 1ns / 1ps

module unary_ops_test_tb;
    reg clk = 0;
    reg resetn;
    reg [3:0] x;

    wire       r_not;
    wire [3:0] r_bitnot;
    wire       r_and;
    wire       r_or;
    wire       r_xor;
    wire       r_nand;
    wire       r_nor;
    wire       r_xnor;

    unary_ops_test uut (
        .clk     (clk),
        .resetn  (resetn),
        .x       (x),
        .r_not   (r_not),
        .r_bitnot(r_bitnot),
        .r_and   (r_and),
        .r_or    (r_or),
        .r_xor   (r_xor),
        .r_nand  (r_nand),
        .r_nor   (r_nor),
        .r_xnor  (r_xnor)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d %0d %0d %0d %0d",
                        r_not, r_bitnot, r_and, r_or, r_xor, r_nand, r_nor, r_xnor);
        end
    endtask

    initial begin
        resetn = 0;
        x = 4'h0;
        sample(); // cycle 1: reset, everything 0

        resetn = 1;

        x = 4'h0;
        sample(); // cycle 2

        x = 4'hF;
        sample(); // cycle 3

        x = 4'h5;
        sample(); // cycle 4

        x = 4'h7;
        sample(); // cycle 5

        x = 4'hA;
        sample(); // cycle 6

        $finish;
    end
endmodule
