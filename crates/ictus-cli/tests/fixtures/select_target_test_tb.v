// Reference testbench for tests/differential_select_target.rs.
`timescale 1ns / 1ps

module select_target_test_tb;
    reg clk = 0;
    reg resetn;
    reg [3:0] data;

    wire [7:0] acc;

    select_target_test uut (
        .clk   (clk),
        .resetn(resetn),
        .data  (data),
        .acc   (acc)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d", acc);
        end
    endtask

    initial begin
        resetn = 0;
        data = 4'h0;
        sample(); // cycle 1: reset, acc = 0x00

        resetn = 1;

        data = 4'hA;
        sample(); // cycle 2: low 0x0->0x1, high loads 0xA -> acc = 0xA1

        data = 4'hB;
        sample(); // cycle 3: low 0x1->0x2, high loads 0xB -> acc = 0xB2

        data = 4'hC;
        repeat (14) sample(); // low nibble walks 0x3..0xF, then wraps to 0x0

        $finish;
    end
endmodule
