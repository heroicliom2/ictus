// Reference testbench for tests/differential_blocking.rs.
//
// The first cycle is an unsampled warm-up. blocking_sees_old_nb reads
// nb_target's pre-edge value, and on the very first edge that value has
// never been written -- 4-state 'x' in Icarus, 0 in Ictus's 2-state
// kernel (decisions.md D6/D19). By the second edge nb_target holds 99 in
// both, so every sampled cycle compares defined values.
`timescale 1ns / 1ps

module blocking_test_tb;
    reg clk = 0;
    reg [7:0] in;

    wire [7:0] chained;
    wire [7:0] nb_sees_blocking;
    wire [7:0] blocking_sees_old_nb;
    wire [7:0] nb_target;

    blocking_test uut (
        .clk                 (clk),
        .in                  (in),
        .chained             (chained),
        .nb_sees_blocking    (nb_sees_blocking),
        .blocking_sees_old_nb(blocking_sees_old_nb),
        .nb_target           (nb_target)
    );

    always #5 clk = ~clk;

    // Advance one cycle without comparing anything (warm-up only).
    task step;
        begin
            @(posedge clk);
            #1;
        end
    endtask

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d",
                        chained, nb_sees_blocking, blocking_sees_old_nb, nb_target);
        end
    endtask

    initial begin
        in = 8'd10;
        step();

        in = 8'd10; sample();
        in = 8'd20; sample();
        in = 8'd30; sample();
        in = 8'd0;  sample();
        in = 8'd255; sample();

        $finish;
    end
endmodule
