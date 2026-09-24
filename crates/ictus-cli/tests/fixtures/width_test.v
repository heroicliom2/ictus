module width_test (
    input  wire       clk,
    input  wire [3:0] a,
    input  wire [3:0] b,
    input  wire [7:0] wide,
    output reg        any_masked,
    output reg  [5:0] concat_and,
    output reg  [5:0] concat_add,
    output reg  [9:0] concat_mixed,
    output reg  [3:0] not_sum
);
    always @(posedge clk) begin
        // picorv32's own shape: a reduction over a bitwise AND, which
        // needs the AND's width to know how many bits to fold.
        any_masked   <= |(a & ~b);

        // As a concatenation operand, a bitwise result takes the wider
        // operand's width -- exact, since neither operand can set a bit
        // above its own width.
        concat_and   <= {a & b, 2'b11};

        // Same rule for arithmetic, where it *does* discard the carry:
        // 4-bit + 4-bit packs 4 bits, not 5.
        concat_add   <= {a + b, 2'b11};

        // Mixed widths: the wider operand wins, so this packs 8 bits.
        concat_mixed <= {wide + a, 2'b11};

        // A unary operator needs the same width, for the same reason.
        not_sum      <= ~(a + b);
    end
endmodule
