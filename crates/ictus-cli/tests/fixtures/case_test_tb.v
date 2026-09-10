// Reference testbench for tests/differential_case.rs.
`timescale 1ns / 1ps

module case_test_tb;
    reg clk = 0;
    reg resetn;
    reg [2:0] sel;
    wire [7:0] result;

    case_test uut (
        .clk   (clk),
        .resetn(resetn),
        .sel   (sel),
        .result(result)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d", result);
        end
    endtask

    initial begin
        resetn = 0;
        sel = 3'd0;
        sample(); // cycle 1: reset

        resetn = 1;

        sel = 3'd0;
        sample(); // cycle 2: arm 0

        sel = 3'd1;
        sample(); // cycle 3: arm 1

        sel = 3'd2;
        sample(); // cycle 4: comma-joined arm (2)

        sel = 3'd3;
        sample(); // cycle 5: comma-joined arm (3)

        sel = 3'd5;
        sample(); // cycle 6: default

        $finish;
    end
endmodule
