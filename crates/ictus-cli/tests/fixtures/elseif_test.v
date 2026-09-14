module elseif_test (
    input  wire       clk,
    input  wire       resetn,
    input  wire [1:0] level,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        if (!resetn)
            result <= 8'h00;
        else if (level == 2'd0)
            result <= 8'h11;
        else if (level == 2'd1)
            result <= 8'h22;
        else if (level == 2'd2)
            result <= 8'h33;
        else
            result <= 8'hFF;
    end
endmodule
