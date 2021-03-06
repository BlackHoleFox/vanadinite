#![feature(asm, naked_functions, start, lang_items)]
#![no_std]

#[no_mangle]
unsafe extern "C" fn _start() {
    extern "C" {
        fn main(_: isize, _: *const *const u8) -> isize;
    }

    #[rustfmt::skip]
    asm!("
        .align 4
        .option push
        .option norelax
        lla gp, __global_pointer$
        .option pop
    ");

    main(0, 0 as *const *const u8);

    #[rustfmt::skip]
    asm!("
        mv a1, a0
        li a0, 0
        ecall
    ", options(noreturn));
}

#[lang = "start"]
fn lang_start<T>(main: fn() -> T, _argc: isize, _argv: *const *const u8) -> isize {
    main();
    0
}
