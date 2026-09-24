module negedge_test (
    input  wire       clk,
    input  wire [7:0] a,
    output reg  [7:0] out
);
    always @(negedge clk) begin
        out <= a;
    end
endmodule
