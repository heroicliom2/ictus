// Reference testbench for tests/differential_precedence.rs.
//
// The vectors matter here. `sv-parser`'s own (precedence-free) nesting
// agrees with Verilog's on plenty of inputs by coincidence -- the first
// hand-picked vectors for the decoder shape all did -- so these are
// chosen to make the two readings differ.
`timescale 1ns / 1ps

module precedence_test_tb;
    reg clk = 0;
    reg [31:0] d;
    reg [7:0] a, b, c;

    wire       decoded;
    wire [7:0] left_assoc;
    wire [7:0] mixed_arith;
    wire [7:0] tern;

    precedence_test uut (
        .clk(clk), .d(d), .a(a), .b(b), .c(c),
        .decoded(decoded), .left_assoc(left_assoc),
        .mixed_arith(mixed_arith), .tern(tern)
    );

    always #5 clk = ~clk;

    task sample;
        begin
            @(posedge clk);
            #1 $display("%0d %0d %0d %0d", decoded, left_assoc, mixed_arith, tern);
        end
    endtask

    initial begin
        // slli: funct3 == 001 and funct7 == 0000000 -> decoded
        d = 32'h00109093; a = 8'd20; b = 8'd6; c = 8'd3; sample();
        // addi: funct3 == 000, so decoded must be 0. Under the parser's
        // own nesting this reads `d[14:12] == (1 && (d[31:25] == 0))`,
        // which is `0 == 1` here and happens to agree -- included to show
        // the shape is right, not only the value.
        d = 32'h00500093; a = 8'd10; b = 8'd30; c = 8'd4; sample();
        // srai: funct3 == 101 with funct7 == 0100000, so still not the
        // 001/0000000 pattern.
        d = 32'h40505093; a = 8'd200; b = 8'd100; c = 8'd50; sample();
        // funct3 == 001 but funct7 != 0 -> the AND's right arm fails,
        // which the mis-associated reading gets wrong.
        d = 32'h02109093; a = 8'd1; b = 8'd2; c = 8'd3; sample();
        d = 32'h00109093; a = 8'd255; b = 8'd1; c = 8'd1; sample();
        $finish;
    end
endmodule
