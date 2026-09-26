module generate_param_test #(
    parameter WIDE = 1
) (
    input  wire [7:0] a,
    output wire [7:0] y
);
    generate if (WIDE) begin
        localparam SHIFT = 1;
        assign y = a << SHIFT;
    end else begin
        localparam SHIFT = 2;
        assign y = a << SHIFT;
    end endgenerate
endmodule
