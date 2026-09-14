// Reference testbench for tests/differential_dynsel.rs. Holds `data`
// fixed and cycles `idx` through every bit position, so each cycle checks
// a different bit of the same known pattern.
`timescale 1ns / 1ps

module dynsel_test_tb;
    reg clk = 0;
    reg [7:0] data;
    reg [2:0] idx;
    wire bit_out;

    dynsel_test uut (
        .clk    (clk),
        .data   (data),
        .idx    (idx),
        .bit_out(bit_out)
    );

    always #5 clk = ~clk;

    integer i;
    initial begin
        data = 8'b1011_0010;

        for (i = 0; i < 8; i = i + 1) begin
            idx = i;
            @(posedge clk);
            #1 $display("%0d", bit_out);
        end

        $finish;
    end
endmodule
