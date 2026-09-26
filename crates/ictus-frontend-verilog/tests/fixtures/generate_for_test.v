module generate_for_test (
    input  wire [3:0] a,
    output wire [3:0] y
);
    genvar i;
    generate for (i = 0; i < 4; i = i + 1) begin : bits
        assign y[i] = ~a[i];
    end endgenerate
endmodule
