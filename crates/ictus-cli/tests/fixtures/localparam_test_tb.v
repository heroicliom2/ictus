// Reference testbench for tests/differential_localparam.rs.
`timescale 1ns / 1ps

module localparam_test_tb;
    reg clk = 0;
    reg resetn;

    wire [7:0] index_bits_out;
    wire       with_feature_out;
    wire [7:0] wide_reg_out;

    localparam_test uut (
        .clk             (clk),
        .resetn          (resetn),
        .index_bits_out  (index_bits_out),
        .with_feature_out(with_feature_out),
        .wide_reg_out    (wide_reg_out)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d", index_bits_out, with_feature_out, wide_reg_out);
        end
    endtask

    initial begin
        resetn = 0;
        sample(); // cycle 1: reset

        resetn = 1;
        sample(); // cycle 2: index_bits_out=7, with_feature_out=1, wide_reg bit 6 set -> 64
        sample(); // cycle 3: same (bit 6 already set)

        $finish;
    end
endmodule
