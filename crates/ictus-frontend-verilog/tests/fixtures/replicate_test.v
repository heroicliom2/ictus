module replicate_test (
    input  wire       clk,
    input  wire       resetn,
    input  wire [1:0] a,
    input  wire [1:0] b,
    input  wire       flag,
    output reg  [7:0] masked,
    output reg  [7:0] doubled
);
    always @(posedge clk) begin
        if (!resetn) begin
            masked  <= 8'h00;
            doubled <= 8'h00;
        end else begin
            // Single-expr replication, mirroring picorv32's own
            // `mem_wstrb <= mem_la_wstrb & {4{mem_la_write}};` style.
            masked <= 8'hFF & {8{flag}};
            // Multi-part inner concatenation, replicated.
            doubled <= {2{a, b}};
        end
    end
endmodule
