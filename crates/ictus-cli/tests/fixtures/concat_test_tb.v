// Reference testbench for tests/differential_concat.rs.
`timescale 1ns / 1ps

module concat_test_tb;
    reg clk = 0;
    reg resetn;
    reg [3:0] hi;
    reg [3:0] lo;
    wire [7:0] combined;
    wire [8:0] with_flag;

    concat_test uut (
        .clk      (clk),
        .resetn   (resetn),
        .hi       (hi),
        .lo       (lo),
        .combined (combined),
        .with_flag(with_flag)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d", combined, with_flag);
        end
    endtask

    initial begin
        resetn = 0;
        hi = 4'h0;
        lo = 4'h0;
        sample(); // cycle 1: reset

        resetn = 1;

        hi = 4'hA;
        lo = 4'hB;
        sample(); // cycle 2: {hi,lo} = 0xAB, {1,hi,lo} = 0x1AB

        hi = 4'hF;
        lo = 4'h0;
        sample(); // cycle 3: {hi,lo} = 0xF0, {1,hi,lo} = 0x1F0

        hi = 4'h0;
        lo = 4'hF;
        sample(); // cycle 4: {hi,lo} = 0x0F, {1,hi,lo} = 0x10F

        $finish;
    end
endmodule
