module task_call_args_test (
    input  wire       clk,
    output reg  [7:0] count
);
    task bump(input [7:0] amount);
        count = count + amount;
    endtask

    always @(posedge clk) begin
        bump(1);
    end
endmodule
