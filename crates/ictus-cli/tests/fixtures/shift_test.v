module shift_test (
    input  wire        clk,
    input  wire [7:0]  value,
    input  wire [2:0]  amount,
    output reg  [7:0]  shl_out,
    output reg  [7:0]  shr_out,
    output reg  [7:0]  ashr_out,
    output reg  [15:0] ashr_wide_out,
    output reg  [7:0]  ashr_unsigned_out
);
    // Mirrors picorv32's own styles directly: `reg_op1 << reg_op2[4:0]`,
    // `reg_op1 >> 4`, and `$signed(reg_op1) >>> 4` -- the last being the
    // only form where `>>>` differs from `>>` at all.
    always @(posedge clk) begin
        shl_out       <= value << amount;
        shr_out       <= value >> amount;
        ashr_out      <= $signed(value) >>> amount;
        // Same expression into a *wider* target: Verilog sign-extends to
        // the context width first, so the extra bits must come out as
        // sign bits too, not zeros.
        ashr_wide_out <= $signed(value) >>> amount;
        // `>>>` on an *unsigned* operand is an ordinary logical shift per
        // the LRM -- so this must match shr_out, not ashr_out.
        ashr_unsigned_out <= value >>> amount;
    end
endmodule
