module task_call_test (
    input  wire       clk,
    input  wire       resetn,
    output reg  [7:0] count
);
    // Mirrors picorv32's own style directly: an empty task used as a
    // no-op placeholder (there, for a compiled-out `assert(...)`).
    task empty_statement;
        begin end
    endtask

    always @(posedge clk) begin
        if (!resetn) begin
            count <= 8'h00;
        end else begin
            empty_statement;
            count <= count + 8'h01;
        end
    end
endmodule
