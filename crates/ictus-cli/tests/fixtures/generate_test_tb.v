// Reference testbench for tests/differential_generate.rs.
//
// Samples are taken twice per cycle: once just after the inputs change
// ("M", mid-cycle, no edge yet) and once just after the following edge
// ("P"). The mid-cycle sample is the one that matters. After an edge a
// registered `sum <= a + b` and a combinational `sum = a + b` agree, so a
// simulator that wrongly lowered *both* generate branches would pass
// every post-edge comparison; only between edges does the registered
// version still show the previous value.
`timescale 1ns / 1ps

module generate_test_tb;
    reg clk = 0;
    reg [7:0] a, b;

    wire [7:0] sum;
    wire [7:0] diff;
    wire [7:0] picked;

    generate_test uut (
        .clk(clk), .a(a), .b(b), .sum(sum), .diff(diff), .picked(picked)
    );

    always #5 clk = ~clk;

    task mid;
        begin
            #1 $display("M %0d %0d %0d", sum, diff, picked);
        end
    endtask

    task post;
        begin
            @(posedge clk);
            #1 $display("P %0d %0d %0d", sum, diff, picked);
        end
    endtask

    initial begin
        // Warm-up edge, unsampled: the registered `sum` reads x until its
        // first edge in Icarus and 0 here (decisions.md D19).
        a = 8'd1; b = 8'd2;
        @(posedge clk); #1;

        a = 8'd10;  b = 8'd3;   mid(); post();
        a = 8'd200; b = 8'd100; mid(); post();
        a = 8'd5;   b = 8'd9;   mid(); post();
        $finish;
    end
endmodule
