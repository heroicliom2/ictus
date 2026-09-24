module comb_always_test (
    input  wire       clk,
    input  wire [1:0] sel,
    input  wire [7:0] a,
    input  wire [7:0] b,
    input  wire       en,
    output reg  [7:0] muxed,
    output reg        flag,
    output reg  [7:0] held,
    output reg  [7:0] chained,
    output reg  [7:0] latched
);
    // Verilog's dominant combinational idiom: assign a default, then
    // override it. Every pass over this block writes both signals twice,
    // which is exactly why settling can't decide it has converged by
    // asking each write whether it changed something.
    always @* begin
        muxed = 8'd0;
        flag  = 1'b0;
        case (sel)
            2'd0: muxed = a;
            2'd1: muxed = b;
            2'd2: begin
                muxed = a + b;
                flag  = 1'b1;
            end
            default: muxed = 8'hFF;
        endcase
    end

    // Declared *before* the block it reads from, so a single pass in
    // source order would compute it from a stale `muxed`.
    always @* begin
        chained = muxed + 8'd1;
    end

    // No assignment when `en` is low, so the previous value persists --
    // Verilog's inferred latch.
    always @* begin
        if (en)
            held = a;
    end

    always @(posedge clk) begin
        latched <= chained;
    end
endmodule
