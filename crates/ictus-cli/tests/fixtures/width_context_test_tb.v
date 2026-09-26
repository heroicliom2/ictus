// Reference testbench for tests/differential_width_context.rs.
`timescale 1ns / 1ps

module width_context_test_tb;
    reg clk = 0;
    reg [7:0] a, b;

    wire [7:0]  shifted, neg_shifted, shamt, sum;
    wire        below_max, wrapped_eq, add_gt, add_land, add_not;
    wire [15:0] not16, subshr16, signed_add16, neg_signed16;
    wire [9:0]  shl_cat;
    wire        cout, case_carry, casez_wide, casez_short;

    width_context_test uut (
        .clk(clk), .a(a), .b(b),
        .shifted(shifted), .below_max(below_max), .neg_shifted(neg_shifted),
        .wrapped_eq(wrapped_eq), .add_gt(add_gt), .add_land(add_land),
        .add_not(add_not), .shamt(shamt), .not16(not16), .subshr16(subshr16),
        .signed_add16(signed_add16), .neg_signed16(neg_signed16), .shl_cat(shl_cat), .cout(cout), .sum(sum),
        .case_carry(case_carry), .casez_wide(casez_wide), .casez_short(casez_short)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d",
                        shifted, below_max, neg_shifted, wrapped_eq, add_gt, add_land,
                        add_not, shamt, not16, subshr16, signed_add16, neg_signed16, shl_cat, cout, sum,
                        case_carry, casez_wide, casez_short);
        end
    endtask

    initial begin
        a = 8'd3;   b = 8'd5;   sample();  // 3 - 5 wraps
        a = 8'd9;   b = 8'd4;   sample();  // nothing wraps
        a = 8'd0;   b = 8'd1;   sample();  // wraps to 255
        a = 8'd201; b = 8'd100; sample();  // sum carries out
        a = 8'd255; b = 8'd1;   sample();  // sum wraps to exactly 0
        a = 8'd128; b = 8'd128; sample();  // likewise, and both signed-negative
        a = 8'd0;   b = 8'd255; sample();  // shift amount wraps to 1
        a = 8'd200; b = 8'd100; sample();  // 300: the case item
        a = 8'd0;   b = 8'd196; sample();  // casez: top byte zero, b = 1100_0100
        a = 8'd3;   b = 8'd200; sample();  // casez: top byte non-zero
        $finish;
    end
endmodule
