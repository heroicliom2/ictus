module inst_unknown_port_test (input wire [3:0] a, output wire [3:0] y);
    passthru u (.in(a), .outt(y));
endmodule
module passthru (input wire [3:0] in, output wire [3:0] out);
    assign out = in;
endmodule
