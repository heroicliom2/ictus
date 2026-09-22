module replicate_variable_count_test (
    input  wire       clk,
    input  wire [2:0] n,
    input  wire       flag,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        result <= {n{flag}};
    end
endmodule
