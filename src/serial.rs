use core::sync::atomic::AtomicBool;

use spin::{Mutex, MutexGuard};
use uart_16550::SerialPort;
use x86_64::instructions::interrupts::without_interrupts;

static SERIAL1: Mutex<SerialPort> = Mutex::new(unsafe { SerialPort::new(0x3F8) });
static INIT: AtomicBool = AtomicBool::new(false);

fn serial1() -> MutexGuard<'static, SerialPort> {
    if !INIT.swap(true, core::sync::atomic::Ordering::Relaxed) {
        let mut guard = SERIAL1.lock();
        guard.init();
        guard
    } else {
        SERIAL1.lock()
    }
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;

    // avoid deadlocks by disabling interrupts before aquiring the lock,
    // enabling interrupts after lock is released.
    without_interrupts(|| {
        serial1()
            .write_fmt(args)
            .expect("Printing to serial failed");
    })
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! sprint {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

/// Prints to the host through the serial interface, appending a newline.
#[macro_export]
macro_rules! sprintln {
    () => ($crate::sprint!("\n"));
    ($fmt:expr) => ($crate::serial::_print(format_args_nl!($fmt)));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial::_print(
        format_args_nl!($fmt, $($arg)*)
    ));
}
