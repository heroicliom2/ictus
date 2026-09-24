module blocking_test (
    input  wire       clk,
    input  wire [7:0] in,
    output reg  [7:0] chained,
    output reg  [7:0] nb_sees_blocking,
    output reg  [7:0] blocking_sees_old_nb,
    output reg  [7:0] nb_target
);
    reg [7:0] b;

    // Mirrors picorv32's own mixing of the two kinds in one clocked block
    // (`set_mem_do_rinst = 1;` sitting alongside `decoder_trigger <= 0;`).
    always @(posedge clk) begin
        // Blocking: lands immediately, so the next statement sees it.
        b = in + 8'd1;
        chained = b + 8'd1;

        // A non-blocking right-hand side also sees the new b...
        nb_sees_blocking <= b;

        // ...but a non-blocking write itself is not visible to a later
        // blocking read in the same edge: this must read nb_target's
        // *pre-edge* value, not the 99 queued on the line above.
        nb_target <= 8'd99;
        blocking_sees_old_nb = nb_target;
    end
endmodule
