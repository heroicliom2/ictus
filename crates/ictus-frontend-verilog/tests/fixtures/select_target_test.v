module select_target_test (
    input  wire       clk,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        result[3:0] <= 4'hF;
    end
endmodule
