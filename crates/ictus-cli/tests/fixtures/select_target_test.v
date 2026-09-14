module select_target_test (
    input  wire       clk,
    input  wire       resetn,
    input  wire [3:0] data,
    output reg  [7:0] acc
);
    // Two disjoint partial writes to the same register within one clock
    // edge -- the low nibble self-increments (and must wrap at 4 bits,
    // not bleed into the high nibble), the high nibble loads from `data`.
    // Exercises the kernel's read-modify-write commit logic, including
    // multiple partial writes to one signal combining correctly in a
    // single tick.
    always @(posedge clk) begin
        if (!resetn) begin
            acc <= 8'h00;
        end else begin
            acc[3:0] <= acc[3:0] + 4'h1;
            acc[7:4] <= data;
        end
    end
endmodule
