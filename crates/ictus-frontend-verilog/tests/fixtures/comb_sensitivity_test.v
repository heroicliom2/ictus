module comb_sensitivity_test (
    input  wire [7:0] a,
    input  wire [7:0] b,
    output reg  [7:0] out
);
    always @(a or b) begin
        out = a + b;
    end
endmodule
