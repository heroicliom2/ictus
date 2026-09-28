// A blocking assignment to a concatenation whose right-hand side reads a
// signal that an *earlier* part of the target writes. Verilog evaluates
// `{y, x}` once, before either part is written, so this swaps; writing the
// parts one at a time would set both to `y`. Must be rejected.
module blocking_concat_swap_test (
    input [3:0] a,
    input [3:0] b,
    output reg [3:0] x,
    output reg [3:0] y
);
    always @* begin
        x = a;
        y = b;
        {x, y} = {y, x};
    end
endmodule
