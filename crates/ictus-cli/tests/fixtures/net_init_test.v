module net_init_test (
    input  wire       clk,
    input  wire [7:0] a,
    input  wire [7:0] b,
    output wire [7:0] sum,
    output wire       both,
    output reg  [7:0] latched
);
    // A net declaration carrying an initializer is a continuous
    // assignment -- picorv32 drives most of its combinational logic this
    // way rather than with standalone `assign` statements.
    wire [7:0] doubled = a + a;
    wire       a_big   = a > b;

    assign sum  = doubled + b;
    assign both = a_big && b != 8'd0;

    always @(posedge clk) begin
        latched <= doubled;
    end
endmodule
