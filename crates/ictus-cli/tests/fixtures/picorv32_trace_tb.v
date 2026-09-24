// Reference testbench for tests/differential_picorv32.rs -- the whole
// vendored picorv32 design, not a hand-written fixture.
//
// Ictus can't instantiate modules, so it cannot run this testbench. That
// is the point of the arrangement: this side runs in Icarus and *records*
// what it drove into picorv32 and what picorv32 produced, and the Rust
// side replays the recorded inputs into Ictus and compares the outputs.
// The stimulus is then identical by construction, and any difference is
// unambiguously a difference in how the two simulators interpreted the
// same design under the same inputs -- rather than a difference between
// two hand-written memory models that were supposed to match.
//
// Sampling happens at each *negedge*. Everything here is posedge-driven,
// so between a negedge and the next posedge nothing moves: the values
// recorded at negedge N are simultaneously picorv32's outputs after edge
// N and the inputs it will see at edge N+1. One extra sample is taken
// before the first posedge to give the replay its starting inputs.
//
// No parameter overrides on the instantiation, deliberately: Ictus has no
// way to override a parameter at instantiation, so both sides must run
// the design's own defaults or they would not be comparing the same
// thing.
`timescale 1ns / 1ps

module picorv32_trace_tb;
    reg clk = 0;
    reg resetn = 0;
    reg mem_ready = 0;
    reg [31:0] mem_rdata = 0;

    wire        trap;
    wire        mem_valid;
    wire        mem_instr;
    wire [31:0] mem_addr;
    wire [31:0] mem_wdata;
    wire [ 3:0] mem_wstrb;
    wire        mem_la_read;
    wire        mem_la_write;
    wire [31:0] mem_la_addr;
    wire [31:0] mem_la_wdata;
    wire [ 3:0] mem_la_wstrb;

    wire        pcpi_valid;
    wire [31:0] pcpi_insn;
    wire [31:0] pcpi_rs1;
    wire [31:0] pcpi_rs2;
    wire [31:0] eoi;
    wire        trace_valid;
    wire [35:0] trace_data;

    picorv32 uut (
        .clk         (clk),
        .resetn      (resetn),
        .trap        (trap),
        .mem_valid   (mem_valid),
        .mem_instr   (mem_instr),
        .mem_ready   (mem_ready),
        .mem_addr    (mem_addr),
        .mem_wdata   (mem_wdata),
        .mem_wstrb   (mem_wstrb),
        .mem_rdata   (mem_rdata),
        .mem_la_read (mem_la_read),
        .mem_la_write(mem_la_write),
        .mem_la_addr (mem_la_addr),
        .mem_la_wdata(mem_la_wdata),
        .mem_la_wstrb(mem_la_wstrb),
        .pcpi_valid  (pcpi_valid),
        .pcpi_insn   (pcpi_insn),
        .pcpi_rs1    (pcpi_rs1),
        .pcpi_rs2    (pcpi_rs2),
        .pcpi_wr     (1'b0),
        .pcpi_rd     (32'b0),
        .pcpi_wait   (1'b0),
        .pcpi_ready  (1'b0),
        .irq         (32'b0),
        .eoi         (eoi),
        .trace_valid (trace_valid),
        .trace_data  (trace_data)
    );

    always #5 clk = ~clk;

    // A hand-assembled RISC-V program:
    //
    //   0:  addi x1, x0, 5      ; x1 = 5
    //   4:  addi x2, x0, 7      ; x2 = 7
    //   8:  add  x3, x1, x2     ; x3 = 12
    //   12: jal  x0, 0          ; spin here forever
    //
    // Register-to-register work and a jump, deliberately no load or
    // store. Not because those are uninteresting -- they are the most
    // interesting part -- but because picorv32 computes a store's write
    // data in an `always @*` block, which this frontend does not yet
    // lower at all. Reaching a store today would compare Ictus against
    // Icarus on logic Ictus never ran, which tests nothing and reports it
    // as a failure of something else. The program grows the moment
    // `always @*` lands; see docs/roadmap.md.
    reg [31:0] memory [0:63];
    integer i;
    initial begin
        for (i = 0; i < 64; i = i + 1)
            memory[i] = 32'h00000013;   // nop (addi x0, x0, 0)
        memory[0] = 32'h00500093;
        memory[1] = 32'h00700113;
        memory[2] = 32'h002081b3;
        memory[3] = 32'h0000006f;
    end

    // A single-cycle-latency memory: one wait state, then ready for
    // exactly one cycle. Writes are applied whole-word, since the test
    // program performs none and a strobe-accurate model would only add
    // untested behaviour.
    always @(posedge clk) begin
        mem_ready <= 0;
        if (mem_valid && !mem_ready) begin
            mem_ready <= 1;
            mem_rdata <= memory[mem_addr[31:2]];
            if (mem_wstrb != 0)
                memory[mem_addr[31:2]] <= mem_wdata;
        end
    end

    // Deassert reset after a few cycles.
    initial begin
        resetn = 0;
        repeat (4) @(posedge clk);
        resetn <= 1;
    end

    task record;
        begin
            $display("%0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d",
                     resetn, mem_ready, mem_rdata,
                     trap, mem_valid, mem_instr, mem_addr, mem_wdata, mem_wstrb,
                     mem_la_read, mem_la_write);
        end
    endtask

    initial begin
        #1 record();               // before the first posedge
        forever begin
            @(negedge clk);
            record();
        end
    end

    integer r;
    initial begin
        repeat (80) @(posedge clk);
        // Final architectural state. The port trace shows the core
        // behaving; this shows it actually computed something -- and it
        // reads picorv32's register file, which is an unpacked array, so
        // it exercises that support against a real design rather than a
        // fixture.
        for (r = 0; r < 8; r = r + 1)
            $display("R %0d %0d", r, uut.cpuregs[r]);
        $finish;
    end
endmodule
