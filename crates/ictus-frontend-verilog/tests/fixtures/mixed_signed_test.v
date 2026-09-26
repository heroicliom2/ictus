module mixed_signed_test (
    input  wire        clk,
    input  wire [7:0]  a,
    input  wire [7:0]  b,
    output reg  [15:0] y
);
    always @(posedge clk) begin
        y <= $signed(a) + b;
    end
endmodule
