#![feature(abi_x86_interrupt)]
#![feature(allocator_api)] // *sigh*. why does the new major version of `acpi` require this now?
#![feature(format_args_nl)]
#![no_std]
#![no_main]

use core::time::Duration;

use time::delay;

extern crate alloc;

mod bitmap;
mod draw;
mod interrupts;
mod mem;
mod serial;
pub mod setup;
mod time;
mod utils;

#[no_mangle]
pub extern "C" fn kernel_start() -> ! {
    sprintln!("im alive");
    setup::init();
    sprintln!("huh");
    for _ in 0..8 {
        println!(" 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0");
        println!("0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 ");
    }
    loop {}
}
