module param_test #(
    parameter [7:0] OFFSET = 8'd5,
    parameter [7:0] LIMIT = 8'd12
) (
    input  wire       clk,
    input  wire       resetn,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        if (!resetn)
            result <= 8'h00;
        else if (result >= LIMIT)
            result <= OFFSET;
        else
            result <= result + OFFSET;
    end
endmodule
