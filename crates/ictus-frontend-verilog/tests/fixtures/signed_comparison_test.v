module signed_comparison_test (
    input  wire       clk,
    input  wire [7:0] a,
    input  wire [7:0] b,
    output reg        lt
);
    always @(posedge clk) begin
        lt <= $signed(a) < $signed(b);
    end
endmodule
