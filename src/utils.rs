/// Do not execute until the next interrupt. Makes CPU work less harder.
pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt()
    }
}
