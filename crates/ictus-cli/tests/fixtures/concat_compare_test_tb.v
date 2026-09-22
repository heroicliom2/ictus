// Reference testbench for tests/differential_concat_compare.rs.
`timescale 1ns / 1ps

module concat_compare_test_tb;
    reg clk = 0;
    reg [3:0] a;
    reg [3:0] b;

    wire [3:0] flags;

    concat_compare_test uut (
        .clk  (clk),
        .a    (a),
        .b    (b),
        .flags(flags)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d", flags);
        end
    endtask

    initial begin
        a = 4'h5;
        b = 4'h5;
        sample(); // cycle 1: a==b -> flags = {1,0,0,0} = 4'b1000 = 8

        a = 4'h3;
        b = 4'h7;
        sample(); // cycle 2: a<b -> flags = {0,1,0,1} = 4'b0101 = 5

        a = 4'h9;
        b = 4'h2;
        sample(); // cycle 3: a>b -> flags = {0,0,1,1} = 4'b0011 = 3

        $finish;
    end
endmodule
