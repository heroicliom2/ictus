// Reference testbench for tests/differential_width.rs.
//
// The point of comparison here is Verilog's self-determined width rule
// for a binary operator: the wider operand's width, with the result
// truncated back to it. For `a + b` with 4-bit operands that *discards
// the carry*, which is the case most worth checking against a real
// simulator rather than against my own reading of the LRM.
`timescale 1ns / 1ps

module width_test_tb;
    reg clk = 0;
    reg [3:0] a;
    reg [3:0] b;
    reg [7:0] wide;

    wire       any_masked;
    wire [5:0] concat_and;
    wire [5:0] concat_add;
    wire [9:0] concat_mixed;
    wire [3:0] not_sum;

    width_test uut (
        .clk         (clk),
        .a           (a),
        .b           (b),
        .wide        (wide),
        .any_masked  (any_masked),
        .concat_and  (concat_and),
        .concat_add  (concat_add),
        .concat_mixed(concat_mixed),
        .not_sum     (not_sum)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d %0d",
                        any_masked, concat_and, concat_add, concat_mixed, not_sum);
        end
    endtask

    initial begin
        a = 4'd12; b = 4'd10; wide = 8'd200; sample();
        a = 4'd15; b = 4'd15; wide = 8'd255; sample();
        a = 4'd0;  b = 4'd0;  wide = 8'd0;   sample();
        a = 4'd5;  b = 4'd3;  wide = 8'd100; sample();
        $finish;
    end
endmodule
