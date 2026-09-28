// An array element as one part of a concatenation target. Must be
// rejected, not taken as a bit-select of the array signal.
module concat_target_array_part_test (
    input clk,
    input [7:0] in,
    output reg [3:0] x
);
    reg [3:0] mem [0:3];
    always @(posedge clk) begin
        {mem[0], x} <= in;
    end
endmodule
