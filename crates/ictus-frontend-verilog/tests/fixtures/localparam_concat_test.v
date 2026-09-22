module localparam_concat_test (
    input  wire       clk,
    input  wire       resetn,
    output reg  [7:0] trace_out
);
    // Mirrors picorv32's own style directly:
    // `localparam [35:0] TRACE_BRANCH = {4'b 0001, 32'b 0};`
    localparam [7:0] TRACE_X = {4'b0001, 4'b0010};

    always @(posedge clk) begin
        if (!resetn) begin
            trace_out <= 8'h00;
        end else begin
            trace_out <= TRACE_X;
        end
    end
endmodule
