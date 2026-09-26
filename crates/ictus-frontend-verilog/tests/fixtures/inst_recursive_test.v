module inst_recursive_test (input wire [3:0] a, output wire [3:0] y);
    looped u (.in(a), .out(y));
endmodule
module looped (input wire [3:0] in, output wire [3:0] out);
    looped again (.in(in), .out(out));
endmodule
