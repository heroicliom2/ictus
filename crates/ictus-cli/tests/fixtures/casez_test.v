module casez_test (
    input  wire       clk,
    input  wire [3:0] sel,
    output reg  [7:0] result
);
    always @(posedge clk) begin
        casez (sel)
            4'b1???: result <= 8'hA0; // wildcard: top bit set (sel 8-15)
            4'b01??: result <= 8'hB0; // wildcard: bits 3:2 == 01 (sel 4-7)
            4'd0:    result <= 8'hC0; // exact match mixed into the same casez
            default: result <= 8'hFF;
        endcase
    end
endmodule
