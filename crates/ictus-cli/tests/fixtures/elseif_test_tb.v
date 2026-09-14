// Reference testbench for tests/differential_elseif.rs.
`timescale 1ns / 1ps

module elseif_test_tb;
    reg clk = 0;
    reg resetn;
    reg [1:0] level;
    wire [7:0] result;

    elseif_test uut (
        .clk   (clk),
        .resetn(resetn),
        .level (level),
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
        level = 2'd0;
        sample(); // cycle 1: reset

        resetn = 1;

        level = 2'd0;
        sample(); // cycle 2: first else-if

        level = 2'd1;
        sample(); // cycle 3: second else-if

        level = 2'd2;
        sample(); // cycle 4: third else-if

        level = 2'd3;
        sample(); // cycle 5: final else (falls through all conditions)

        $finish;
    end
endmodule
