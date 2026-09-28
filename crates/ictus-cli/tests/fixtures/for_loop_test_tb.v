module for_loop_test_tb;
    reg clk = 0;
    reg rst = 1;
    reg [7:0] a = 0, b = 0;
    wire [7:0] reversed, prefix_xor, acc, ext_signed, ext_unsigned;
    wire [3:0] popcount;
    wire [15:0] nibble_swap, nibble_not;
    wire [4:0] pair_matches;
    wire [5:0] index_sum;
    wire [8:0] carry_dropped, carry_kept;

    for_loop_test uut (
        .clk(clk), .rst(rst), .a(a), .b(b), .reversed(reversed),
        .popcount(popcount), .nibble_swap(nibble_swap), .nibble_not(nibble_not),
        .prefix_xor(prefix_xor), .pair_matches(pair_matches), .index_sum(index_sum),
        .acc(acc), .ext_signed(ext_signed), .ext_unsigned(ext_unsigned),
        .carry_dropped(carry_dropped), .carry_kept(carry_kept)
    );

    task step(input [7:0] va, input [7:0] vb);
        begin
            rst = 0;
            a = va;
            b = vb;
            #1 clk = 1;
            #1 clk = 0;
            #1 $display("%0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d",
                va, vb, reversed, popcount, nibble_swap, nibble_not, prefix_xor,
                pair_matches, index_sum, acc, ext_signed, ext_unsigned, carry_dropped,
                carry_kept);
        end
    endtask

    initial begin
        // Reset, unsampled.
        #1 clk = 1;
        #1 clk = 0;
        step(8'h00, 8'h00);
        step(8'hFF, 8'hFF);
        step(8'hA5, 8'h3C);
        step(8'h01, 8'h80);
        step(8'h96, 8'h69);
        step(8'h7E, 8'h18);
        step(8'h80, 8'h01);
        step(8'h5A, 8'h5A);
    end
endmodule
