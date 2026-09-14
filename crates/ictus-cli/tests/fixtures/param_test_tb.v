// Reference testbench for tests/differential_param.rs.
`timescale 1ns / 1ps

module param_test_tb;
    reg clk = 0;
    reg resetn;
    wire [7:0] result;

    param_test uut (
        .clk   (clk),
        .resetn(resetn),
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
        sample(); // cycle 1: reset -> 0

        resetn = 1;
        sample(); // cycle 2: 0 -> 5
        sample(); // cycle 3: 5 -> 10
        sample(); // cycle 4: 10 -> 15
        sample(); // cycle 5: 15 >= LIMIT(12) -> back to OFFSET(5)
        sample(); // cycle 6: 5 -> 10

        $finish;
    end
endmodule
