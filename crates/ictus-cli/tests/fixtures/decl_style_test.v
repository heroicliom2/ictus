module decl_style_test (
    input clk, resetn,
    output reg [7:0] result
);
    reg [7:0] a, b, c;

    always @(posedge clk) begin
        if (!resetn) begin
            a <= 8'h00;
            b <= 8'h00;
            c <= 8'h00;
        end else begin
            a <= 8'd5;
            b <= 8'd9;
            c <= 8'd7;
        end
        result <= (a > b) ? (a > c ? a : c) : (b > c ? b : c);
    end
endmodule
