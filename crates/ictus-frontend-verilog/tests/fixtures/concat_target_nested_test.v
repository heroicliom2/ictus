module concat_target_nested_test (
    input  wire       clk,
    input  wire [1:0] x,
    output reg  [1:0] a,
    output reg        b,
    output reg        c
);
    always @(posedge clk) begin
        {a, {b, c}} <= {x, 1'b0};
    end
endmodule
