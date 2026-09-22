module concat_compare_test (
    input  wire       clk,
    input  wire [3:0] a,
    input  wire [3:0] b,
    output reg  [3:0] flags
);
    // Mirrors real picorv32 usage: comparison/logical results
    // concatenated together to build a combined flags/decode signal.
    always @(posedge clk) begin
        flags <= {a == b, a < b, a > b, a != b};
    end
endmodule
