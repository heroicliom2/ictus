module blocking_compound_test (
    input  wire       clk,
    input  wire [7:0] in,
    output reg  [7:0] acc
);
    always @(posedge clk) begin
        acc += in;
    end
endmodule
