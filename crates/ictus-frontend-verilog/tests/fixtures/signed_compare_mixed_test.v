module signed_compare_mixed_test (
    input  wire       clk,
    input  wire [7:0] a,
    input  wire [7:0] b,
    output reg        lt
);
    always @(posedge clk) begin
        lt <= ($signed(a) < b);
    end
endmodule
