module array_port_test (
    input  wire       clk,
    output reg  [7:0] mem [0:3]
);
    always @(posedge clk) begin
        mem[0] <= 8'h01;
    end
endmodule
