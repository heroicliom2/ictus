module replicate_zero_count_test (
    input  wire       clk,
    input  wire       flag,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        result <= {0{flag}};
    end
endmodule
