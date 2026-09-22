module signed_ext_test (
    input  wire        clk,
    input  wire        resetn,
    input  wire [5:0]  narrow,
    output reg  [11:0] wide
);
    always @(posedge clk) begin
        if (!resetn) begin
            wide <= 12'h000;
        end else begin
            wide <= $signed(narrow);
        end
    end
endmodule
