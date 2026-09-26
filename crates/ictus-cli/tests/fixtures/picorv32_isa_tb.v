// Reference testbench for tests/differential_picorv32_isa.rs: runs one of
// picorv32's own per-instruction tests (riscv-tests rv32ui, assembled by
// bench/isa/build.sh) on the vendored picorv32, and records the run for
// Ictus to replay.
//
// Same arrangement as picorv32_trace_tb.v: Icarus runs this, recording
// what it drove into the core and what the core produced; the Rust side
// replays the recorded inputs into Ictus and compares. The program image
// is chosen at *run* time with `+program=<file.hex>`, so this compiles
// once for all 37 tests.
//
// Output lines, each tagged so the Rust side can tell them apart:
//   T ...  one trace row per negedge (inputs, then outputs)
//   C n    a character the program printed (a store to 0x10000000)
//   R i v  register x<i> after the run
//
// The memory honours byte strobes. The first testbench didn't need to,
// since its program only stored whole words; the `sb` and `sh` tests
// would fail against a whole-word memory in *Icarus*, and a reference run
// that fails its own test tells you nothing about Ictus.
`timescale 1ns / 1ps

module picorv32_isa_tb;
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

    // No parameter overrides: Ictus can't override a top-level parameter,
    // so both sides must run picorv32's defaults.
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

    // 4 KiB -- the largest test image is about 1.6 KiB.
    reg [31:0] memory [0:1023];
    reg [1023:0] program_file;
    integer i;
    initial begin
        for (i = 0; i < 1024; i = i + 1)
            memory[i] = 32'h00000013;   // nop, so a stray fetch is harmless
        if (!$value$plusargs("program=%s", program_file)) begin
            $display("missing +program=<file.hex>");
            $finish;
        end
        $readmemh(program_file, memory);
    end

    // One wait state, then ready for exactly one cycle. A store to
    // 0x10000000 is the tests' character output and is reported rather
    // than written; everything else addresses the 4 KiB memory.
    always @(posedge clk) begin
        mem_ready <= 0;
        if (mem_valid && !mem_ready) begin
            mem_ready <= 1;
            if (mem_addr == 32'h1000_0000) begin
                if (mem_wstrb != 0)
                    $display("C %0d", mem_wdata[7:0]);
                mem_rdata <= 0;
            end else begin
                mem_rdata <= memory[mem_addr[11:2]];
                if (mem_wstrb[0]) memory[mem_addr[11:2]][ 7: 0] <= mem_wdata[ 7: 0];
                if (mem_wstrb[1]) memory[mem_addr[11:2]][15: 8] <= mem_wdata[15: 8];
                if (mem_wstrb[2]) memory[mem_addr[11:2]][23:16] <= mem_wdata[23:16];
                if (mem_wstrb[3]) memory[mem_addr[11:2]][31:24] <= mem_wdata[31:24];
            end
        end
    end

    initial begin
        resetn = 0;
        repeat (4) @(posedge clk);
        resetn <= 1;
    end

    task record;
        begin
            $display("T %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d",
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

    // Every test ends in `ebreak` -- a failing one directly, a passing one
    // via the start stub -- which raises `trap`. Run a few cycles past it,
    // with a cap in case the core never gets there.
    //
    // `trap !== 1'b1`, not `!trap`: before reset takes effect `trap` is
    // x, `!x` is x, and a `while` whose condition is x doesn't run -- so
    // the obvious spelling ends the simulation at time zero.
    integer cycles;
    integer r;
    initial begin
        cycles = 0;
        while (trap !== 1'b1 && cycles < 20000) begin
            @(posedge clk);
            cycles = cycles + 1;
        end
        repeat (4) @(posedge clk);
        for (r = 1; r < 32; r = r + 1)
            $display("R %0d %0d", r, uut.cpuregs[r]);
        $finish;
    end
endmodule
