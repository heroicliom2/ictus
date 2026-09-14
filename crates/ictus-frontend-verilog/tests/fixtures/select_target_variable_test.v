module select_target_variable_test (
    input  wire       clk,
    input  wire [2:0] i,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        result[i] <= 1'b1;
    end
endmodule
