// Reference testbench for tests/differential_signed_concat.rs.
`timescale 1ns / 1ps

module signed_concat_test_tb;
    reg clk = 0;
    reg resetn;
    reg sign_bit;
    reg [4:0] rest;

    wire [11:0] wide;

    signed_concat_test uut (
        .clk     (clk),
        .resetn  (resetn),
        .sign_bit(sign_bit),
        .rest    (rest),
        .wide    (wide)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d", wide);
        end
    endtask

    initial begin
        resetn = 0;
        sign_bit = 1'b0;
        rest = 5'h00;
        sample(); // cycle 1: reset, wide = 0x000

        resetn = 1;

        sign_bit = 1'b0;
        rest = 5'b11111;
        sample(); // cycle 2: {0, 11111} = +31, sign bit clear -> wide = 0x01F

        sign_bit = 1'b1;
        rest = 5'b00000;
        sample(); // cycle 3: {1, 00000} = -32, sign bit set -> wide = 0xFE0

        sign_bit = 1'b1;
        rest = 5'b11111;
        sample(); // cycle 4: {1, 11111} = -1, sign bit set -> wide = 0xFFF

        $finish;
    end
endmodule
