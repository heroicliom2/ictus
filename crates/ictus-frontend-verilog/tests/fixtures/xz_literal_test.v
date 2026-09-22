module xz_literal_test (
    input  wire        clk,
    input  wire        resetn,
    input  wire [3:0]  data,
    output reg  [7:0]  bin_out,
    output reg  [7:0]  hex_out,
    output reg  [7:0]  dec_out,
    output reg  [7:0]  mixed_out,
    output reg  [7:0]  concat_out
);
    // Mirrors picorv32's own style directly: `assign pcpi_mul_rd = 32'bx;`,
    // `decoded_imm <= 1'bx;`, and `{16'bx, mem_16bit_buffer}` -- an
    // all-`x` literal (any base), a *mixed* literal (`x` alongside real
    // digits), and an `x`-containing concatenation operand.
    always @(posedge clk) begin
        if (!resetn) begin
            bin_out    <= 8'h00;
            hex_out    <= 8'h00;
            dec_out    <= 8'h00;
            mixed_out  <= 8'h00;
            concat_out <= 8'h00;
        end else begin
            bin_out    <= 8'bx;
            hex_out    <= 8'hxx;
            dec_out    <= 8'dx;
            mixed_out  <= 4'b10x1;
            concat_out <= {4'bx, data};
        end
    end
endmodule
