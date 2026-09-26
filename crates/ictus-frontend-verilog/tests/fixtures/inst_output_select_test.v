module inst_output_select_test (input wire [3:0] a, output wire [7:0] y);
    passthru u (.in(a), .out(y[3:0]));
endmodule
module passthru (input wire [3:0] in, output wire [3:0] out);
    assign out = in;
endmodule
