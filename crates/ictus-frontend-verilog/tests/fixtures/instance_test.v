// The top is the first module in the file; the others exist to be
// instantiated.
module instance_test #(
    parameter W = 8
) (
    input  wire         clk,
    input  wire         resetn,
    input  wire [W-1:0] a,
    input  wire [W-1:0] b,
    output wire [W-1:0] sum_q,
    output wire [W-1:0] diff_q,
    output wire [3:0]   narrow_q,
    output wire [W-1:0] count_a,
    output wire [W-1:0] last_sum
);
    // Two instances of one module, whose internal signals must not
    // collide. Each port is connected a different way:
    //   .clk(clk)     a whole signal of the same width -- aliased
    //   .in(a + b)    an expression -- a copy of the port, driven by it
    //   .q(sum_q)     an output to a whole signal -- aliased
    // and the width is passed down from this module's own parameter.
    accum #(.WIDTH(W)) u_sum (
        .clk   (clk),
        .resetn(resetn),
        .in    (a + b),
        .q     (sum_q),
        .count (count_a),
        .last  (last_sum)
    );

    accum #(.WIDTH(W)) u_diff (
        .clk   (clk),
        .resetn(resetn),
        .in    (a - b),
        .q     (diff_q),
        .count (),
        .last  ()
    );

    // An output wider than what it's connected to: Verilog truncates, via
    // a copy of the port and a continuous assignment into `narrow_q`.
    accum #(.WIDTH(W)) u_narrow (
        .clk   (clk),
        .resetn(resetn),
        .in    (a),
        .q     (narrow_q),
        .count (),
        .last  ()
    );
endmodule

module accum #(
    parameter WIDTH = 4
) (
    input  wire             clk,
    input  wire             resetn,
    input  wire [WIDTH-1:0] in,
    output reg  [WIDTH-1:0] q,
    output wire [WIDTH-1:0] count,
    output wire [WIDTH-1:0] last
);
    // An internal signal: every instance gets its own.
    reg [WIDTH-1:0] prev;
    assign last = prev;

    // A grandchild, so instance names nest and aliases chain through two
    // levels (`value` -> `count` -> the top's `count_a`).
    counter #(.WIDTH(WIDTH)) u_count (
        .clk   (clk),
        .resetn(resetn),
        .value (count)
    );

    always @(posedge clk) begin
        if (!resetn) begin
            q <= 0;
            prev <= 0;
        end else begin
            q <= q + in;
            prev <= in;
        end
    end
endmodule

module counter #(
    parameter WIDTH = 4
) (
    input  wire             clk,
    input  wire             resetn,
    output reg  [WIDTH-1:0] value
);
    always @(posedge clk) begin
        if (!resetn)
            value <= 0;
        else
            value <= value + 1;
    end
endmodule
