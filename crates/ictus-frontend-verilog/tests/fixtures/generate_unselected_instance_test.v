module generate_unselected_instance_test #(
    parameter ENABLE_EXTRA = 0
) (
    input  wire [7:0] a,
    output wire [7:0] y
);
    // picorv32's `generate if (ENABLE_MUL)` shape: an instance in the
    // branch that isn't selected. It must neither be lowered nor rejected
    // -- it doesn't exist in the elaborated design.
    generate if (ENABLE_EXTRA) begin
        some_extra_unit extra (.a(a), .y(y));
    end else begin
        assign y = a;
    end endgenerate
endmodule
