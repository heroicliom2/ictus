module inst_drives_input_test (input wire [3:0] a, output wire [3:0] y);
    // The child output is wired onto this module's own input port.
    passthru u (.in(y), .out(a));
endmodule
module passthru (input wire [3:0] in, output wire [3:0] out);
    assign out = in;
endmodule
