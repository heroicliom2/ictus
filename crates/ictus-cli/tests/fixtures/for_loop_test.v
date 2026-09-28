// `for` loops, which Ictus unrolls while lowering (docs/decisions.md D33),
// with the indexed part-selects and `$unsigned` that picorv32's multiplier
// uses alongside them. Each always block has its own loop variable: an
// `always @*` is sensitive to every variable it reads, its loop variable
// included, so sharing one between blocks would make them retrigger each
// other in an event-driven simulator.
module for_loop_test (
    input clk,
    input rst,
    input [7:0] a,
    input [7:0] b,
    output reg [7:0] reversed,
    output reg [3:0] popcount,
    output reg [15:0] nibble_swap,
    output reg [15:0] nibble_not,
    output reg [7:0] prefix_xor,
    output reg [4:0] pair_matches,
    output reg [5:0] index_sum,
    output reg [7:0] acc,
    output [7:0] ext_signed,
    output [7:0] ext_unsigned,
    output [8:0] carry_dropped,
    output [8:0] carry_kept
);
    integer i, c, j, k, m, p, q, s, r;
    wire [15:0] ab = {a, b};

    // A bit-select on both sides, the target's index being the loop
    // variable and the source's computed from it.
    always @* begin
        for (i = 0; i < 8; i = i + 1)
            reversed[i] = a[7 - i];
    end

    // Accumulating across iterations.
    always @* begin
        popcount = 0;
        for (c = 0; c < 8; c = c + 1)
            popcount = popcount + a[c];
    end

    // `+:` on both sides, with a computed base.
    always @* begin
        for (j = 0; j < 4; j = j + 1)
            nibble_swap[(3 - j) * 4 +: 4] = ab[j * 4 +: 4];
    end

    // A descending loop, and `-:` on both sides. It stops at 0 rather than
    // running to -1, which v1 rejects (see lower_for).
    always @* begin
        for (k = 16; k > 0; k = k - 4)
            nibble_not[k - 1 -: 4] = ~ab[k - 1 -: 4];
    end

    // Each iteration reads what the previous one wrote.
    always @* begin
        prefix_xor[0] = a[0];
        for (m = 1; m < 8; m = m + 1)
            prefix_xor[m] = prefix_xor[m - 1] ^ a[m];
    end

    // Nested loops, with the loop variables in a condition.
    always @* begin
        pair_matches = 0;
        for (p = 0; p < 4; p = p + 1)
            for (q = 0; q < 4; q = q + 1)
                if (a[p * 2 +: 2] == b[q * 2 +: 2])
                    pair_matches = pair_matches + 1;
    end

    // The loop variable as a value, not just an index.
    always @* begin
        index_sum = 0;
        for (s = 0; s < 8; s = s + 1)
            if (a[s])
                index_sum = index_sum + s;
    end

    // A loop in a clocked block, with non-blocking writes: four 2-bit
    // accumulators side by side, each wrapping on its own.
    always @(posedge clk) begin
        if (rst)
            acc <= 0;
        else
            for (r = 0; r < 4; r = r + 1)
                acc[r * 2 +: 2] <= acc[r * 2 +: 2] + a[r * 2 +: 2];
    end

    // `$unsigned`: the same bits read as unsigned, at the argument's own
    // width -- so no sign extension, and no carry out of `a + b`.
    assign ext_signed = $signed(a[3:0]);
    assign ext_unsigned = $unsigned($signed(a[3:0]));
    assign carry_dropped = $unsigned(a + b);
    assign carry_kept = a + b;
endmodule
