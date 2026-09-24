// Reference testbench for tests/differential_net_init.rs.
`timescale 1ns / 1ps

module net_init_test_tb;
    reg clk = 0;
    reg [7:0] a, b;

    wire [7:0] sum;
    wire       both;
    wire [7:0] latched;

    net_init_test uut (.clk(clk), .a(a), .b(b), .sum(sum), .both(both), .latched(latched));

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d", sum, both, latched);
        end
    endtask

    initial begin
        a = 8'd3;  b = 8'd4;  sample();
        a = 8'd10; b = 8'd2;  sample();
        a = 8'd10; b = 8'd0;  sample();
        a = 8'd100; b = 8'd200; sample();
        a = 8'd0;  b = 8'd5;  sample();
        $finish;
    end
endmodule
