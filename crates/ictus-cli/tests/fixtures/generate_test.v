module generate_test #(
    parameter REG_SUM = 1,
    parameter REG_DIFF = 0,
    parameter MODE = 2
) (
    input  wire       clk,
    input  wire [7:0] a,
    input  wire [7:0] b,
    output reg  [7:0] sum,
    output reg  [7:0] diff,
    output wire [7:0] picked
);
    // picorv32's `generate if (TWO_CYCLE_ALU)` shape: the same signal
    // driven by a clocked block in one branch and a combinational one in
    // the other. Only the selected branch may exist -- and the two
    // parameters here select *opposite* branches, so a registered and a
    // combinational version end up side by side in one design.
    generate if (REG_SUM) begin
        always @(posedge clk) begin
            sum <= a + b;
        end
    end else begin
        always @* begin
            sum = a + b;
        end
    end endgenerate

    generate if (REG_DIFF) begin
        always @(posedge clk) begin
            diff <= a - b;
        end
    end else begin
        always @* begin
            diff = a - b;
        end
    end endgenerate

    // An `else if` chain whose conditions are expressions over a
    // parameter, not bare names.
    generate if (MODE == 0) begin
        assign picked = a;
    end else if (MODE == 1) begin
        assign picked = b;
    end else begin
        assign picked = a ^ b;
    end endgenerate
endmodule
