// Reference testbench for tests/differential_casez.rs.
`timescale 1ns / 1ps

module casez_test_tb;
    reg clk = 0;
    reg [3:0] sel;
    wire [7:0] result;

    casez_test uut (
        .clk   (clk),
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
        sel = 4'd0;
        sample(); // cycle 1: exact match (4'd0) -> 0xC0

        sel = 4'd2;
        sample(); // cycle 2: no match -> default 0xFF

        sel = 4'd5;
        sample(); // cycle 3: 4'b01?? -> 0xB0

        sel = 4'd9;
        sample(); // cycle 4: 4'b1??? -> 0xA0

        sel = 4'd15;
        sample(); // cycle 5: 4'b1??? boundary -> 0xA0

        $finish;
    end
endmodule
