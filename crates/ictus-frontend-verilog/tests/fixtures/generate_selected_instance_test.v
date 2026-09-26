module generate_selected_instance_test #(
    parameter ENABLE_EXTRA = 1
) (
    input  wire [7:0] a,
    output wire [7:0] y
);
    generate if (ENABLE_EXTRA) begin
        some_extra_unit extra (.a(a), .y(y));
    end else begin
        assign y = a;
    end endgenerate
endmodule
