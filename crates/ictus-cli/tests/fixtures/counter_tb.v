// Reference testbench for the differential test in
// tests/differential_counter.rs. Drives the same reset/count sequence the
// Rust-side test drives through ictus_kernel::Simulation directly, and
// prints one decimal `count` value per line -- one line per clock cycle,
// in cycle order -- so the two traces can be diffed exactly.
`timescale 1ns / 1ps

module counter_tb;
    reg clk = 0;
    reg resetn;
    wire [7:0] count;

    counter uut (
        .clk   (clk),
        .resetn(resetn),
        .count (count)
    );

    always #5 clk = ~clk;

    integer i;
    initial begin
        resetn = 0;

        // Two cycles held in reset.
        @(posedge clk);
        #1 $display("%0d", count);
        @(posedge clk);
        #1 $display("%0d", count);

        // Release reset well before the next edge (no race with the DUT's
        // own non-blocking assignment).
        resetn = 1;

        for (i = 0; i < 10; i = i + 1) begin
            @(posedge clk);
            #1 $display("%0d", count);
        end

        $finish;
    end
endmodule
