// Blocking assignments to a concatenation target -- the forms v1 accepts.
// The right-hand side may read the *last* part's target (nothing has been
// written yet when it is evaluated for that part) or neither part's.
module blocking_concat_target_test (
    input clk,
    input [3:0] a,
    input [3:0] b,
    output reg [3:0] hi,
    output reg [3:0] lo,
    output reg cout,
    output reg [3:0] sum,
    output reg [3:0] q_hi,
    output reg [3:0] q_lo
);
    // Reads `lo`, the last part: lo = b first, then {a, b} + 1.
    always @* begin
        lo = b;
        {hi, lo} = {a, lo} + 8'd1;
    end

    // Reads neither part; the carry must survive the split.
    always @* begin
        {cout, sum} = a + b;
    end

    // The same inside a clocked block, where blocking and the concatenation
    // target meet in picorv32's multiplier's style.
    reg [3:0] t;
    always @(posedge clk) begin
        t = a ^ b;
        {q_hi, q_lo} = {t, a} - 8'd3;
    end
endmodule
