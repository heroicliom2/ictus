// Reference testbench for tests/differential_task_call.rs.
`timescale 1ns / 1ps

module task_call_test_tb;
    reg clk = 0;
    reg resetn;

    wire [7:0] count;

    task_call_test uut (
        .clk   (clk),
        .resetn(resetn),
        .count (count)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d", count);
        end
    endtask

    initial begin
        resetn = 0;
        sample(); // cycle 1: reset, count = 0

        resetn = 1;
        repeat (4) sample(); // cycles 2-5: count = 1, 2, 3, 4

        $finish;
    end
endmodule
