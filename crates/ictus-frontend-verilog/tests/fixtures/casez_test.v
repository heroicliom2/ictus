module casez_test (
    input  wire       clk,
    input  wire [2:0] sel,
    output reg        result
);
    always @(posedge clk) begin
        casez (sel)
            3'b1??:  result <= 1'b1;
            default: result <= 1'b0;
        endcase
    end
endmodule
