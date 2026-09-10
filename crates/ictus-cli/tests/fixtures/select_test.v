module select_test (
    input  wire        clk,
    input  wire        resetn,
    input  wire [15:0] data,
    output reg  [7:0]  low_byte,
    output reg  [7:0]  high_byte,
    output reg         msb_bit
);
    always @(posedge clk) begin
        if (!resetn) begin
            low_byte  <= 8'h00;
            high_byte <= 8'h00;
            msb_bit   <= 1'b0;
        end else begin
            low_byte  <= data[7:0];
            high_byte <= data[15:8];
            msb_bit   <= data[15];
        end
    end
endmodule
