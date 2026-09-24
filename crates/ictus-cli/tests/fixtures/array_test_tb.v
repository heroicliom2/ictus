// Reference testbench for tests/differential_array.rs.
//
// The array is filled during an unsampled warm-up phase before any
// comparison starts. That's deliberate, not incidental: an array element
// that has never been written reads as 4-state 'x' in Icarus but as 0 in
// Ictus's 2-state kernel (decisions.md D6/D19), so sampling one would be
// comparing two different-but-both-valid answers. Every *sampled* cycle
// below reads only elements that have already been written.
`timescale 1ns / 1ps

module array_test_tb;
    reg clk = 0;
    reg resetn;
    reg write_enable;
    reg [1:0] waddr;
    reg [7:0] wdata;
    reg [1:0] raddr;

    wire [7:0] rdata;
    wire [7:0] fixed_read;

    array_test uut (
        .clk         (clk),
        .resetn      (resetn),
        .write_enable(write_enable),
        .waddr       (waddr),
        .wdata       (wdata),
        .raddr       (raddr),
        .rdata       (rdata),
        .fixed_read  (fixed_read)
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
            #1 $display("%0d %0d", rdata, fixed_read);
        end
    endtask

    initial begin
        resetn = 0;
        write_enable = 0;
        waddr = 0;
        wdata = 0;
        raddr = 0;
        step();

        // Fill every element, so nothing sampled below reads an
        // uninitialized one.
        resetn = 1;
        write_enable = 1;
        waddr = 2'd0; wdata = 8'hA1; step();
        waddr = 2'd1; wdata = 8'hB2; step();
        waddr = 2'd2; wdata = 8'hC3; step();
        waddr = 2'd3; wdata = 8'hD4; step();

        // Read each element back in turn. fixed_read is the constant-index
        // read of mem[2] (0xC3) throughout.
        write_enable = 0;
        raddr = 2'd0; sample();
        raddr = 2'd1; sample();
        raddr = 2'd2; sample();
        raddr = 2'd3; sample();

        // Overwrite mem[1] while reading it: the read must see the
        // *pre-edge* value, which is what makes this a non-blocking write.
        write_enable = 1;
        waddr = 2'd1; wdata = 8'h5E; raddr = 2'd1; sample();

        write_enable = 0;
        raddr = 2'd1; sample();   // now the new value
        raddr = 2'd0; sample();   // and the neighbours are untouched

        // Same again on the element the constant-index read watches, so
        // both read paths are seen updating.
        write_enable = 1;
        waddr = 2'd2; wdata = 8'h7F; raddr = 2'd2; sample();

        write_enable = 0;
        raddr = 2'd2; sample();

        $finish;
    end
endmodule
