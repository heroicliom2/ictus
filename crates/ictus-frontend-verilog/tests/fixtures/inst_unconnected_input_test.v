module inst_unconnected_input_test (input wire [3:0] a, output wire [3:0] y);
    passthru u (.out(y));
endmodule
module passthru (input wire [3:0] in, output wire [3:0] out);
    assign out = in;
endmodule
