module reg_initializer_test (
    input  wire       clk,
    output reg  [7:0] out
);
    reg [7:0] count = 8'd3;

    always @(posedge clk) begin
        count <= count + 8'd1;
        out <= count;
    end
endmodule
