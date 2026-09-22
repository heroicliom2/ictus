// Reference testbench for tests/differential_signed_ext.rs.
`timescale 1ns / 1ps

module signed_ext_test_tb;
    reg clk = 0;
    reg resetn;
    reg [5:0] narrow;

    wire [11:0] wide;

    signed_ext_test uut (
        .clk   (clk),
        .resetn(resetn),
        .narrow(narrow),
        .wide  (wide)
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
        narrow = 6'h00;
        sample(); // cycle 1: reset, wide = 0x000

        resetn = 1;

        narrow = 6'b000001;
        sample(); // cycle 2: +1 -> wide = 0x001

        narrow = 6'b011111;
        sample(); // cycle 3: +31, sign bit clear -> wide = 0x01F

        narrow = 6'b100000;
        sample(); // cycle 4: -32, sign bit set -> wide = 0xFE0

        narrow = 6'b111111;
        sample(); // cycle 5: -1, sign bit set -> wide = 0xFFF

        narrow = 6'b110000;
        sample(); // cycle 6: -16, sign bit set -> wide = 0xFF0

        $finish;
    end
endmodule
