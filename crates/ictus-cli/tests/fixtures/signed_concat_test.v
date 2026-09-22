module signed_concat_test (
    input  wire        clk,
    input  wire        resetn,
    input  wire        sign_bit,
    input  wire [4:0]  rest,
    output reg  [11:0] wide
);
    // Mirrors picorv32's own style directly:
    // `mem_rdata_q[31:20] <= $signed({mem_rdata_latched[12], mem_rdata_latched[6:2]});`
    always @(posedge clk) begin
        if (!resetn) begin
            wide <= 12'h000;
        end else begin
            wide <= $signed({sign_bit, rest});
        end
    end
endmodule
