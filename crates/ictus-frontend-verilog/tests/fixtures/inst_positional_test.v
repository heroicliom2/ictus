module inst_positional_test (input wire clk, input wire [3:0] a, output wire [3:0] y);
    passthru u (a, y);
endmodule
module passthru (input wire [3:0] in, output wire [3:0] out);
    assign out = in;
endmodule
