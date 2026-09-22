module xz_default_test (
    input  wire        clk,
    input  wire        resetn,
    input  wire [1:0]  sel,
    output reg  [7:0]  result
);
    // Mirrors picorv32's own "default to don't-care, then override"
    // idiom directly (`decoded_imm <= 1'bx;` followed by a `case` that
    // overrides it for every real instruction encoding). Every possible
    // `sel` value is covered by the case below, so the 8'bx default is
    // never actually observed -- exactly the pattern that makes this
    // safe to differential-test against a 4-state reference simulator at
    // all (see decisions.md D19: the x-resolves-to-0 policy itself isn't
    // differentially testable in general, since a 4-state simulator
    // reports a genuinely-x result as 'x', not 0).
    always @(posedge clk) begin
        if (!resetn) begin
            result <= 8'h00;
        end else begin
            result <= 8'bx;
            case (sel)
                2'b00: result <= 8'h11;
                2'b01: result <= 8'h22;
                2'b10: result <= 8'h33;
                2'b11: result <= 8'h44;
            endcase
        end
    end
endmodule
