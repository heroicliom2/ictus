module case_test (
    input  wire       clk,
    input  wire       resetn,
    input  wire [2:0] sel,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        if (!resetn)
            result <= 8'h00;
        else begin
            case (sel)
                3'd0:       result <= 8'h11;
                3'd1:       result <= 8'h22;
                3'd2, 3'd3: result <= 8'h33;
                default:    result <= 8'hFF;
            endcase
        end
    end
endmodule
