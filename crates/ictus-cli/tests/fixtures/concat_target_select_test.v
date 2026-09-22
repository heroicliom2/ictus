module concat_target_select_test (
    input  wire        clk,
    input  wire        resetn,
    input  wire [7:0]  data,
    output reg         carry,
    output reg  [7:0]  acc
);
    // Mirrors picorv32's own style: a concatenation assignment target
    // whose parts are constant bit-selects/part-selects of the *same*
    // signal, mixed with a plain full-width signal as another part.
    always @(posedge clk) begin
        if (!resetn) begin
            carry <= 1'b0;
            acc   <= 8'h00;
        end else begin
            {carry, acc[7:4], acc[3:0]} <= {1'b1, data[3:0], data[7:4]};
        end
    end
endmodule
