module ops_test (
    input  wire       clk,
    input  wire       resetn,
    input  wire [7:0] a,
    input  wire [7:0] b,
    output reg  [7:0] and_result,
    output reg  [7:0] or_result,
    output reg  [7:0] xor_result,
    output reg        eq_flag,
    output reg        ne_flag,
    output reg        lt_flag,
    output reg        le_flag,
    output reg        gt_flag,
    output reg        ge_flag,
    output reg        logic_flag
);
    reg [7:0] scratch;

    always @(posedge clk) begin
        if (!resetn) begin
            and_result <= 8'h00;
            or_result  <= 8'h00;
            xor_result <= 8'h00;
            eq_flag    <= 1'b0;
            ne_flag    <= 1'b0;
            lt_flag    <= 1'b0;
            le_flag    <= 1'b0;
            gt_flag    <= 1'b0;
            ge_flag    <= 1'b0;
            logic_flag <= 1'b0;
            scratch    <= 8'hFF;
        end else begin
            and_result <= a & b;
            or_result  <= a | b;
            xor_result <= a ^ b;
            eq_flag    <= (a == b);
            ne_flag    <= (a != b);
            lt_flag    <= (a < b);
            le_flag    <= (a <= b);
            gt_flag    <= (a > b);
            ge_flag    <= (a >= b);
            logic_flag <= (eq_flag || ne_flag) && resetn;
            scratch    <= scratch & 8'b0000_1111;
        end
    end
endmodule
