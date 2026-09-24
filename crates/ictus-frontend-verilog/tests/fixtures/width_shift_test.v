module width_shift_test (
    input  wire       clk,
    input  wire [3:0] a,
    output reg  [5:0] out
);
    always @(posedge clk) begin
        out <= {a << 1, 2'b11};
    end
endmodule
