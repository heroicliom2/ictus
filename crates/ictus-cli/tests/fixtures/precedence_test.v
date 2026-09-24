module precedence_test (
    input  wire        clk,
    input  wire [31:0] d,
    input  wire [7:0]  a,
    input  wire [7:0]  b,
    input  wire [7:0]  c,
    output reg         decoded,
    output reg  [7:0]  left_assoc,
    output reg  [7:0]  mixed_arith,
    output reg  [7:0]  tern
);
    always @(posedge clk) begin
        // picorv32's instruction-decoder shape: `==` binds tighter than
        // `&&`, so this is (d[14:12]==1) && (d[31:25]==0).
        decoded <= d[14:12] == 3'b001 && d[31:25] == 7'b0000000;

        // Left-associative: (a - b) - c, not a - (b - c).
        left_assoc <= a - b - c;

        // `*` binds tighter than `+`: a + (b * c).
        mixed_arith <= a + b * c;

        // A bare ternary ends the chain, and everything left of `?` is
        // its condition: (a > b) ? b : c.
        tern <= a > b ? b : c;
    end
endmodule
