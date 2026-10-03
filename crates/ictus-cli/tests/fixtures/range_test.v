module range_test (
    input  wire       clk,
    input  wire [7:0] d,
    input  wire [2:0] i,
    input  wire [8:1] p,            // a port numbered from 1
    output reg        off_bit,      // r[1]
    output reg  [3:0] off_part,     // r[4:1]
    output reg        off_dyn,      // r[i + 1]
    output reg  [3:0] off_up,       // r[2 +: 4]
    output reg        asc_bit,      // q[0]
    output reg  [3:0] asc_part,     // q[0:3]
    output reg        asc_dyn,      // q[i]
    output reg  [3:0] asc_up,       // q[2 +: 4]
    output reg  [7:0] off_written,  // w, written as w[4:1] and w[8:5]
    output reg  [7:0] asc_written,  // v, written as v[0:3] and v[4:7]
    output reg  [7:0] mem_read,     // mem[i[1:0] + 1], elements 1 to 4
    output reg        port_bit      // p[1]
);
    // Ranges that don't start at 0, or that run upwards. In `[8:1]` the
    // least significant bit is index 1; in `[0:7]` it is index 7, and
    // index 0 is the most significant. See docs/decisions.md D34.
    wire [8:1] r = d;
    wire [0:7] q = d;

    reg [8:1] w;
    reg [0:7] v;

    reg [7:0] mem [1:4];

    always @(posedge clk) begin
        off_bit  <= r[1];
        off_part <= r[4:1];
        off_dyn  <= r[i + 1];
        off_up   <= r[2 +: 4];
        asc_bit  <= q[0];
        asc_part <= q[0:3];
        asc_dyn  <= q[i];
        asc_up   <= q[2 +: 4];

        // Written through the same kinds of range, then read back whole:
        // `w` ends up equal to `d`, while `v[0:3]` is its *top* nibble.
        w[4:1] <= d[3:0];
        w[8:5] <= d[7:4];
        v[0:3] <= d[3:0];
        v[4:7] <= d[7:4];
        off_written <= w;
        asc_written <= v;

        mem[i[1:0] + 1] <= d;
        mem_read <= mem[i[1:0] + 1];

        port_bit <= p[1];
    end
endmodule
