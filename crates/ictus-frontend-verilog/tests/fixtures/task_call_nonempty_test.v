module task_call_nonempty_test (
    input  wire       clk,
    output reg  [7:0] count
);
    task bump;
        count = count + 1;
    endtask

    always @(posedge clk) begin
        bump;
    end
endmodule
