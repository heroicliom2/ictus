module wildcard_outside_case_test (
    output wire [7:0] result
);
    // Wildcard bits are only meaningful as a casez/casex match pattern --
    // this design has no well-defined 2-state value to assign here, so
    // v1 must reject it rather than guess.
    assign result = 8'b1010????;
endmodule
