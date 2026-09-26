module width_context_test (
    input  wire        clk,
    input  wire [7:0]  a,
    input  wire [7:0]  b,
    // An operator that reads high bits, applied to arithmetic that wraps.
    output reg  [7:0]  shifted,      // (a - b) >> 1
    output reg         below_max,    // (a - b) < 8'hFF
    output reg  [7:0]  neg_shifted,  // (-a) >> 1
    output reg         wrapped_eq,   // (a - b) == 8'hFE
    output reg         add_gt,       // (a + b) > 8'd250
    output reg         add_land,     // (a + b) && 1'b1
    output reg         add_not,      // !(a + b)
    output reg  [7:0]  shamt,        // 8'd1 << (a - b)
    // A wider target widens the evaluation, so nothing wraps.
    output reg  [15:0] not16,        // ~a
    output reg  [15:0] subshr16,     // (a - b) >> 1
    output reg  [15:0] signed_add16, // $signed(a) + $signed(b)
    output reg  [15:0] neg_signed16, // -$signed(a)
    // Self-determined widths.
    output reg  [9:0]  shl_cat,      // {a << 1, 2'b11}
    // A concatenation target is sized as a whole: the carry reaches cout.
    output reg         cout,
    output reg  [7:0]  sum,
    // case compares at the widest of selector and items.
    output reg         case_carry,   // case (a + b) 9'd300
    output reg         casez_wide,   // casez ({a, b}) 8'b1???????
    output reg         casez_short   // casez (b) 8'b1??
);
    // Every case here would come out differently if an arithmetic result
    // were evaluated on a 64-bit word and only masked when written -- see
    // docs/decisions.md D31 and ictus-frontend-verilog's `width` module.
    always @(posedge clk) begin
        shifted      <= (a - b) >> 1;
        below_max    <= (a - b) < 8'hFF;
        neg_shifted  <= (-a) >> 1;
        wrapped_eq   <= (a - b) == 8'hFE;
        add_gt       <= (a + b) > 8'd250;
        add_land     <= (a + b) && 1'b1;
        add_not      <= !(a + b);
        shamt        <= 8'd1 << (a - b);

        not16        <= ~a;
        subshr16     <= (a - b) >> 1;
        signed_add16 <= $signed(a) + $signed(b);
        neg_signed16 <= -$signed(a);

        shl_cat      <= {a << 1, 2'b11};

        {cout, sum}  <= a + b;

        // The item is 9 bits, so the selector is computed at 9 bits and
        // keeps its carry; at 8 bits, 200 + 100 would wrap to 44.
        case (a + b)
            9'd300:  case_carry <= 1'b1;
            default: case_carry <= 1'b0;
        endcase

        // An 8-bit item against a 16-bit selector is zero-extended: it
        // matches only when the selector's top eight bits are zero.
        casez ({a, b})
            8'b1???????: casez_wide <= 1'b1;
            default:     casez_wide <= 1'b0;
        endcase

        // Three digits, eight bits: the five bits the digits don't reach
        // are zeros that must match, not wildcards.
        casez (b)
            8'b1??:  casez_short <= 1'b1;
            default: casez_short <= 1'b0;
        endcase
    end
endmodule
