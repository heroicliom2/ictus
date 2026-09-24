module array_element_bitselect_test (
    input  wire       clk,
    input  wire [1:0] addr,
    output reg        bit_out
);
    reg [7:0] mem [0:3];

    always @(posedge clk) begin
        bit_out <= mem[addr][3];
    end
endmodule
