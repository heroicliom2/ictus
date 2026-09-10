// Reference testbench for tests/differential_select.rs.
`timescale 1ns / 1ps

module select_test_tb;
    reg clk = 0;
    reg resetn;
    reg [15:0] data;

    wire [7:0] low_byte;
    wire [7:0] high_byte;
    wire msb_bit;

    select_test uut (
        .clk      (clk),
        .resetn   (resetn),
        .data     (data),
        .low_byte (low_byte),
        .high_byte(high_byte),
        .msb_bit  (msb_bit)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d", low_byte, high_byte, msb_bit);
        end
    endtask

    initial begin
        resetn = 0;
        data = 16'h0000;
        sample(); // cycle 1: reset

        resetn = 1;

        data = 16'hABCD;
        sample(); // cycle 2: low=0xCD, high=0xAB, msb=1

        data = 16'h1234;
        sample(); // cycle 3: low=0x34, high=0x12, msb=0

        data = 16'hFFFF;
        sample(); // cycle 4: low=0xFF, high=0xFF, msb=1

        $finish;
    end
endmodule
