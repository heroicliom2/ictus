// Reference testbench for tests/differential_replicate.rs.
`timescale 1ns / 1ps

module replicate_test_tb;
    reg clk = 0;
    reg resetn;
    reg [1:0] a;
    reg [1:0] b;
    reg flag;

    wire [7:0] masked;
    wire [7:0] doubled;

    replicate_test uut (
        .clk    (clk),
        .resetn (resetn),
        .a      (a),
        .b      (b),
        .flag   (flag),
        .masked (masked),
        .doubled(doubled)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d", masked, doubled);
        end
    endtask

    initial begin
        resetn = 0;
        a = 2'b00;
        b = 2'b00;
        flag = 1'b0;
        sample(); // cycle 1: reset, masked = 0, doubled = 0

        resetn = 1;

        flag = 1'b1;
        a = 2'b10;
        b = 2'b01;
        sample(); // cycle 2: masked = 0xFF, doubled = 10_01_10_01 = 0x99

        flag = 1'b0;
        a = 2'b11;
        b = 2'b00;
        sample(); // cycle 3: masked = 0x00, doubled = 11_00_11_00 = 0xCC

        flag = 1'b1;
        a = 2'b00;
        b = 2'b11;
        sample(); // cycle 4: masked = 0xFF, doubled = 00_11_00_11 = 0x33

        $finish;
    end
endmodule
