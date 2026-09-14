// Reference testbench for tests/differential_decl_style.rs.
`timescale 1ns / 1ps

module decl_style_test_tb;
    reg clk = 0;
    reg resetn;
    wire [7:0] result;

    decl_style_test uut (
        .clk   (clk),
        .resetn(resetn),
        .result(result)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d", result);
        end
    endtask

    initial begin
        resetn = 0;
        // Unsampled settling edge: `result` is assigned unconditionally
        // every cycle (outside the reset if/else), so on the very first
        // edge it reads a/b/c before they've *ever* been reset -- genuinely
        // uninitialized (4-state 'x' in Icarus; this kernel is 2-state and
        // has no 'x', so it reads 0 instead, a documented, expected
        // divergence -- see docs/decisions.md D6). Not sampling this edge
        // keeps the comparison to cycles where both simulators agree on
        // what state means, rather than the initial-state question a
        // 2-state kernel can't answer the same way a 4-state one does.
        @(posedge clk);
        #1;

        sample(); // cycle 2: still reset, a/b/c settled to 0 -> result = 0
        resetn = 1;
        sample(); // cycle 3: a/b/c become 5/9/7; result computed from stale (0,0,0) -> 0
        sample(); // cycle 4: result now computed from settled a=5,b=9,c=7 -> max = 9

        $finish;
    end
endmodule
