module comb_test (
    input  wire       clk,
    input  wire       resetn,
    input  wire [7:0] a,
    input  wire [7:0] b,
    output wire [7:0] sum_comb,
    output reg  [7:0] sum_reg,
    output wire       sum_high
);
    // Pure combinational: depends only on inputs.
    assign sum_comb = a + b;
    // Combinational, but depends on a *register* -- exercises settling
    // combinational logic again after clocked processes commit, not just
    // before (see ictus_kernel::Simulation's doc comment).
    assign sum_high = (sum_reg >= 8'd128);

    always @(posedge clk) begin
        if (!resetn)
            sum_reg <= 8'h00;
        else
            sum_reg <= sum_comb;
    end
endmodule
