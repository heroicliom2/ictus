// Reference testbench for tests/differential_concat_target_select.rs.
`timescale 1ns / 1ps

module concat_target_select_test_tb;
    reg clk = 0;
    reg resetn;
    reg [7:0] data;

    wire carry;
    wire [7:0] acc;

    concat_target_select_test uut (
        .clk   (clk),
        .resetn(resetn),
        .data  (data),
        .carry (carry),
        .acc   (acc)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d", carry, acc);
        end
    endtask

    initial begin
        resetn = 0;
        data = 8'h00;
        sample(); // cycle 1: reset, carry = 0, acc = 0x00

        resetn = 1;

        data = 8'hAB;
        sample(); // cycle 2: acc = nibble-swap(0xAB) = 0xBA, carry = 1

        data = 8'h34;
        sample(); // cycle 3: acc = 0x43, carry = 1

        data = 8'hFF;
        sample(); // cycle 4: acc = 0xFF, carry = 1

        data = 8'h0F;
        sample(); // cycle 5: acc = 0xF0, carry = 1

        $finish;
    end
endmodule
