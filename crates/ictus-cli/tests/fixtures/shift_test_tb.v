// Reference testbench for tests/differential_shift.rs.
`timescale 1ns / 1ps

module shift_test_tb;
    reg clk = 0;
    reg [7:0] value;
    reg [2:0] amount;

    wire [7:0]  shl_out;
    wire [7:0]  shr_out;
    wire [7:0]  ashr_out;
    wire [15:0] ashr_wide_out;
    wire [7:0]  ashr_unsigned_out;

    shift_test uut (
        .clk              (clk),
        .value            (value),
        .amount           (amount),
        .shl_out          (shl_out),
        .shr_out          (shr_out),
        .ashr_out         (ashr_out),
        .ashr_wide_out    (ashr_wide_out),
        .ashr_unsigned_out(ashr_unsigned_out)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d %0d",
                        shl_out, shr_out, ashr_out, ashr_wide_out, ashr_unsigned_out);
        end
    endtask

    initial begin
        // 0x80 = -128 signed: the sign bit is set, so the arithmetic and
        // logical right shifts must disagree (0xE0 vs 0x20).
        value = 8'h80;
        amount = 3'd2;
        sample();

        // 0x0F = +15: sign bit clear, so all three right shifts agree.
        value = 8'h0F;
        amount = 3'd1;
        sample();

        // 0xF0 = -16 signed.
        value = 8'hF0;
        amount = 3'd3;
        sample();

        // 0xFF = -1 signed: an arithmetic shift of any amount stays -1.
        value = 8'hFF;
        amount = 3'd7;
        sample();

        $finish;
    end
endmodule
