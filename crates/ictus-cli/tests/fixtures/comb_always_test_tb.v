// Reference testbench for tests/differential_comb_always.rs.
//
// `held` is only assigned when `en` is high, so it keeps its previous
// value otherwise -- Verilog's inferred latch. The sequence drives `en`
// low across a change of `a` specifically to make that persistence
// observable rather than incidental.
`timescale 1ns / 1ps

module comb_always_test_tb;
    reg clk = 0;
    reg [1:0] sel;
    reg [7:0] a, b;
    reg en;

    wire [7:0] muxed;
    wire       flag;
    wire [7:0] held;
    wire [7:0] chained;
    wire [7:0] latched;

    comb_always_test uut (
        .clk(clk), .sel(sel), .a(a), .b(b), .en(en),
        .muxed(muxed), .flag(flag), .held(held),
        .chained(chained), .latched(latched)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d %0d", muxed, flag, held, chained, latched);
        end
    endtask

    initial begin
        // Warm-up: `held` and `latched` have never been written, so they
        // read x in Icarus and 0 here (decisions.md D19). One cycle with
        // en high gives both a defined value before any comparison.
        sel = 2'd0; a = 8'd11; b = 8'd22; en = 1'b1;
        @(posedge clk); #1;

        sel = 2'd0; a = 8'd11; b = 8'd22; en = 1'b1; sample();
        sel = 2'd1; a = 8'd11; b = 8'd22; en = 1'b1; sample();
        sel = 2'd2; a = 8'd11; b = 8'd22; en = 1'b1; sample();
        sel = 2'd3; a = 8'd11; b = 8'd22; en = 1'b1; sample();
        // en low across a change of `a`: `held` must keep 11.
        sel = 2'd0; a = 8'd99; b = 8'd22; en = 1'b0; sample();
        sel = 2'd2; a = 8'd99; b = 8'd1;  en = 1'b0; sample();
        // en high again: `held` picks up the new `a`.
        sel = 2'd0; a = 8'd99; b = 8'd1;  en = 1'b1; sample();
        $finish;
    end
endmodule
