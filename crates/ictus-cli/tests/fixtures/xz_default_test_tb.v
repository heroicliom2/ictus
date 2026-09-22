// Reference testbench for tests/differential_xz_default.rs.
`timescale 1ns / 1ps

module xz_default_test_tb;
    reg clk = 0;
    reg resetn;
    reg [1:0] sel;

    wire [7:0] result;

    xz_default_test uut (
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
        sel = 2'b00;
        sample(); // cycle 1: reset, result = 0

        resetn = 1;

        sel = 2'b00;
        sample(); // cycle 2: result = 0x11

        sel = 2'b01;
        sample(); // cycle 3: result = 0x22

        sel = 2'b10;
        sample(); // cycle 4: result = 0x33

        sel = 2'b11;
        sample(); // cycle 5: result = 0x44

        $finish;
    end
endmodule
