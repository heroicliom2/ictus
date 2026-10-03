// Reference testbench for tests/differential_range.rs.
//
// The first four edges are an unsampled warm-up: they write every element
// of `mem` (indices 1 to 4) and give `w` and `v` defined values, which
// would otherwise read as 4-state `x` in Icarus and 0 in Ictus
// (decisions.md D19).
`timescale 1ns / 1ps

module range_test_tb;
    reg clk = 0;
    reg [7:0] d;
    reg [2:0] i;
    reg [8:1] p;

    wire       off_bit, off_dyn, asc_bit, asc_dyn, port_bit;
    wire [3:0] off_part, off_up, asc_part, asc_up;
    wire [7:0] off_written, asc_written, mem_read;

    range_test uut (
        .clk(clk), .d(d), .i(i), .p(p),
        .off_bit(off_bit), .off_part(off_part), .off_dyn(off_dyn), .off_up(off_up),
        .asc_bit(asc_bit), .asc_part(asc_part), .asc_dyn(asc_dyn), .asc_up(asc_up),
        .off_written(off_written), .asc_written(asc_written), .mem_read(mem_read),
        .port_bit(port_bit)
    );

    always #5 clk = ~clk;

    task step;
        begin
            @(posedge clk);
            #1;
        end
    endtask

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d",
                        off_bit, off_part, off_dyn, off_up, asc_bit, asc_part,
                        asc_dyn, asc_up, off_written, asc_written, mem_read, port_bit);
        end
    endtask

    initial begin
        p = 8'd0;
        d = 8'h11; i = 3'd0; step();
        d = 8'h22; i = 3'd1; step();
        d = 8'h33; i = 3'd2; step();
        d = 8'h44; i = 3'd3; step();

        d = 8'b1000_0001; i = 3'd0; p = 8'd1; sample();
        d = 8'b0000_0010; i = 3'd1; p = 8'd2; sample();
        d = 8'b1111_0000; i = 3'd3; p = 8'd3; sample();
        d = 8'b0101_0110; i = 3'd6; p = 8'd5; sample();
        d = 8'b1100_1010; i = 3'd5; p = 8'd0; sample();
        d = 8'b0011_1001; i = 3'd2; p = 8'd255; sample();
        $finish;
    end
endmodule
