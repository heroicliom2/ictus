/* One contiguous image from address 0: the start stub first, then the
   test's code, then its data. Load/store tests keep their test vectors in
   .data, and a flat image with no gaps is what lets the testbench load it
   with a plain $readmemh. */
SECTIONS
{
	. = 0;
	.text : { *(.text.start) *(.text*) }
	.data : { *(.rodata*) *(.data*) *(.sdata*) }
	.bss  : { *(.bss*) *(.sbss*) }
}
