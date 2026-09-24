module array_test (
    input  wire       clk,
    input  wire       resetn,
    input  wire       write_enable,
    input  wire [1:0] waddr,
    input  wire [7:0] wdata,
    input  wire [1:0] raddr,
    output reg  [7:0] rdata,
    output reg  [7:0] fixed_read
);
    // Mirrors picorv32's own register file:
    // `reg [31:0] cpuregs [0:regfile_size-1];`, written as
    // `cpuregs[latched_rd] <= cpuregs_wrdata;` and read by index.
    reg [7:0] mem [0:3];

    always @(posedge clk) begin
        if (!resetn) begin
            rdata      <= 8'h00;
            fixed_read <= 8'h00;
        end else begin
            if (write_enable) begin
                mem[waddr] <= wdata;
            end
            // Runtime-indexed read, and a constant-indexed one -- both are
            // element reads on an array, not bit-selects.
            rdata      <= mem[raddr];
            fixed_read <= mem[2];
        end
    end
endmodule
