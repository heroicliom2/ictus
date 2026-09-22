module localparam_test (
    input  wire        clk,
    input  wire        resetn,
    output reg  [7:0]  index_bits_out,
    output reg         with_feature_out,
    output wire [7:0]  wide_reg_out
);
    parameter ENABLE_WIDE = 1;
    parameter ENABLE_EXTRA = 1;

    // Mirrors picorv32's own style directly:
    // `localparam integer regindex_bits = (ENABLE_REGS_16_31 ? 5 : 4) +
    // ENABLE_IRQ*ENABLE_IRQ_QREGS;` -- cross-parameter references, the
    // ternary operator, and multiplication, all in one localparam value.
    localparam integer index_bits = (ENABLE_WIDE ? 5 : 4) + ENABLE_EXTRA*2;
    // Mirrors picorv32's `localparam WITH_PCPI = ENABLE_PCPI || ENABLE_MUL
    // || ...;` -- logical OR of parameters.
    localparam WITH_FEATURE = ENABLE_WIDE || ENABLE_EXTRA;

    // Mirrors picorv32's `reg [regindex_bits-1:0] decoded_rd, decoded_rs1;`
    // -- a packed-range bound that references a localparam.
    reg [index_bits-1:0] wide_reg;
    assign wide_reg_out = wide_reg;

    always @(posedge clk) begin
        if (!resetn) begin
            index_bits_out   <= 8'h00;
            with_feature_out <= 1'b0;
            wide_reg         <= 0;
        end else begin
            index_bits_out   <= index_bits;
            with_feature_out <= WITH_FEATURE;
            // Mirrors picorv32's `decoded_rs1[regindex_bits-1] <= 1;` --
            // a bit-select *target* index that references a localparam.
            wide_reg[index_bits-1] <= 1'b1;
        end
    end
endmodule
