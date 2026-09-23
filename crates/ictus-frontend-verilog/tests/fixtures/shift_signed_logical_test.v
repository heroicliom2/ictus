module shift_signed_logical_test (
    input  wire       clk,
    input  wire [7:0] value,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        result <= $signed(value) >> 2;
    end
endmodule
