module concat_nonblocking_target_test (
    input  wire       clk,
    input  wire [3:0] x,
    output reg  [1:0] a,
    output reg  [1:0] b
);
    always @(posedge clk) begin
        {a, b} <= x;
    end
endmodule
