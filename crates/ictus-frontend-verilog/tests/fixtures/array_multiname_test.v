module array_multiname_test (
    input  wire clk,
    output reg  dummy
);
    reg [7:0] scalar_one, mem [0:3];

    always @(posedge clk) begin
        dummy <= 1'b0;
    end
endmodule
