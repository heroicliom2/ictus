module concat_test (
    input  wire       clk,
    input  wire       resetn,
    input  wire [3:0] hi,
    input  wire [3:0] lo,
    output reg  [7:0] combined,
    output reg  [8:0] with_flag
);
    always @(posedge clk) begin
        if (!resetn) begin
            combined  <= 8'h00;
            with_flag <= 9'h000;
        end else begin
            combined  <= {hi, lo};
            with_flag <= {1'b1, hi, lo};
        end
    end
endmodule
