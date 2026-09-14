module dynsel_test (
    input  wire       clk,
    input  wire [7:0] data,
    input  wire [2:0] idx,
    output reg        bit_out
);
    always @(posedge clk) begin
        bit_out <= data[idx];
    end
endmodule
