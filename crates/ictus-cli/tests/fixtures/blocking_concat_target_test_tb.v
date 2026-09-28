module blocking_concat_target_test_tb;
    reg clk = 0;
    reg [3:0] a, b;
    wire [3:0] hi, lo, sum, q_hi, q_lo;
    wire cout;

    blocking_concat_target_test uut (
        .clk(clk), .a(a), .b(b), .hi(hi), .lo(lo), .cout(cout), .sum(sum),
        .q_hi(q_hi), .q_lo(q_lo)
    );

    task step(input [3:0] va, input [3:0] vb);
        begin
            a = va;
            b = vb;
            #1 clk = 1;
            #1 clk = 0;
            #1 $display("%0d %0d %0d %0d %0d %0d", hi, lo, cout, sum, q_hi, q_lo);
        end
    endtask

    initial begin
        step(4'd3, 4'd9);
        step(4'd15, 4'd15);
        step(4'd0, 4'd0);
        step(4'd8, 4'd7);
        step(4'd0, 4'd2);
        step(4'd12, 4'd5);
    end
endmodule
