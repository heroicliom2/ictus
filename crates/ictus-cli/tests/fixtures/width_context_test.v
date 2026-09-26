module width_context_test (
    input  wire       clk,
    input  wire [7:0] a,
    input  wire [7:0] b,
    output reg  [7:0] shifted,
    output reg        below_max,
    output reg  [7:0] neg_shifted,
    output reg        wrapped_eq
);
    // Each of these applies an operator that looks at *high* bits -- a
    // right shift, a comparison -- to an arithmetic result that wraps
    // around its 8-bit width. Verilog evaluates the arithmetic at 8 bits
    // (IEEE 1800 11.6, context-determined width), so the wrap happens
    // first. See docs/decisions.md D30.
    always @(posedge clk) begin
        shifted     <= (a - b) >> 1;
        below_max   <= (a - b) < 8'hFF;
        neg_shifted <= (-a) >> 1;
        wrapped_eq  <= (a - b) == 8'hFE;
    end
endmodule
