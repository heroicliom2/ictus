module unary_ops_test (
    input  wire       clk,
    input  wire       resetn,
    input  wire [3:0] x,
    output reg        r_not,
    output reg  [3:0] r_bitnot,
    output reg        r_and,
    output reg        r_or,
    output reg        r_xor,
    output reg        r_nand,
    output reg        r_nor,
    output reg        r_xnor
);
    // r_nand mirrors picorv32's own style directly:
    // `(~&mem_rdata_latched[1:0] && mem_xfer));` -- reduction NAND on a
    // multi-bit select.
    always @(posedge clk) begin
        if (!resetn) begin
            r_not    <= 1'b0;
            r_bitnot <= 4'h0;
            r_and    <= 1'b0;
            r_or     <= 1'b0;
            r_xor    <= 1'b0;
            r_nand   <= 1'b0;
            r_nor    <= 1'b0;
            r_xnor   <= 1'b0;
        end else begin
            r_not    <= !x;
            r_bitnot <= ~x;
            r_and    <= &x;
            r_or     <= |x;
            r_xor    <= ^x;
            r_nand   <= ~&x;
            r_nor    <= ~|x;
            r_xnor   <= ~^x;
        end
    end
endmodule
