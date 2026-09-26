module param_override_test #(
    parameter W = 4,
    parameter [0:0] FLAG = 0
) (
    input  wire         clk,
    input  wire [W-1:0] a,
    output reg  [W-1:0] y
);
    // Derived from W, so an override of W must reach it: a localparam
    // can't be overridden itself, but it is recomputed from one that was.
    localparam MAX = (1 << W) - 1;

    always @(posedge clk) begin
        y <= FLAG ? MAX : a;
    end
endmodule
