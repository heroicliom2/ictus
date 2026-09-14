module concat_assign_target_test (
    input  wire [3:0] x,
    output wire [1:0] a,
    output wire [1:0] b
);
    assign {a, b} = x;
endmodule
