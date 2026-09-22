module localparam_unknown_ref_test (
    input  wire clk,
    output reg  flag
);
    localparam BAD = UNDEFINED_PARAM + 1;

    always @(posedge clk) begin
        flag <= BAD;
    end
endmodule
