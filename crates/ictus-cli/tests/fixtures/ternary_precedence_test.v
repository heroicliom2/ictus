module ternary_precedence_test (
    input  wire       clk,
    input  wire [7:0] a,
    input  wire [7:0] c,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        result <= a > c ? a : c;
    end
endmodule
