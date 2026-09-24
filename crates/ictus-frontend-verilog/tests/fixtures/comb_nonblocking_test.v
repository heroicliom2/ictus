module comb_nonblocking_test (
    input  wire [7:0] a,
    output reg  [7:0] out
);
    always @* begin
        out <= a + 8'd1;
    end
endmodule
