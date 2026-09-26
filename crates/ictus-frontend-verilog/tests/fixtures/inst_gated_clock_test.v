module inst_gated_clock_test (input wire clk, input wire en, input wire [3:0] a, output reg [3:0] y, output wire [3:0] z);
    // The child is clocked by a gated version of clk, which v1 cannot
    // tick correctly alongside the parent.
    always @(posedge clk) y <= a;
    flop u (.clk(clk & en), .d(a), .q(z));
endmodule
module flop (input wire clk, input wire [3:0] d, output reg [3:0] q);
    always @(posedge clk) q <= d;
endmodule
