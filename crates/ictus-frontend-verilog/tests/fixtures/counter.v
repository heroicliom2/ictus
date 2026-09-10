module counter (
    input  wire clk,
    input  wire resetn,
    output reg [7:0] count
);
    always @(posedge clk) begin
        if (!resetn)
            count <= 8'd0;
        else
            count <= count + 8'd1;
    end
endmodule
