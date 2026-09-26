module multi_driver_test (
    input  wire       clk,
    input  wire [7:0] a,
    input  wire [7:0] b,
    output reg  [7:0] y
);
    // The shape picorv32's ALU had before `generate if` was elaborated:
    // one signal driven by a clocked block *and* a combinational one.
    always @(posedge clk) begin
        y <= a + b;
    end

    always @* begin
        y = a - b;
    end
endmodule
