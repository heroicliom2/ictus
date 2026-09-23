module signed_compare_test (
    input  wire       clk,
    input  wire [7:0] a,
    input  wire [7:0] b,
    output reg        lt,
    output reg        gt,
    output reg        le,
    output reg        ge,
    output reg        unsigned_lt
);
    // Mirrors picorv32's own ALU line directly:
    // `alu_lts <= $signed(reg_op1) < $signed(reg_op2);`
    // The comparisons are parenthesized so the expression-level `<=`
    // can't be misread against the statement-level assignment `<=`.
    always @(posedge clk) begin
        lt <= ($signed(a) <  $signed(b));
        gt <= ($signed(a) >  $signed(b));
        le <= ($signed(a) <= $signed(b));
        ge <= ($signed(a) >= $signed(b));
        // Control: the same operands compared *unsigned*, which must
        // disagree with `lt` whenever exactly one operand's sign bit is
        // set.
        unsigned_lt <= (a < b);
    end
endmodule
