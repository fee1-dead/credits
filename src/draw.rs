use alloc::boxed::Box;
use alloc::vec;
use core::{fmt, slice};

use limine::framebuffer::Framebuffer;
use spin::Mutex;
use x86_64::instructions::interrupts::without_interrupts;

use crate::bitmap::{to_bitmap, FONT_HEIGHT, FONT_WIDTH};

pub static FBMAN: Mutex<Option<FrameBufferManager>> = Mutex::new(None);

pub fn init(fbman: FrameBufferManager) {
    let mut guard = FBMAN.lock();
    debug_assert!(guard.is_none());

    *guard = Some(fbman);
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;

    // avoid deadlocks by disabling interrupts before aquiring the lock,
    // enabling interrupts after lock is released.
    without_interrupts(|| {
        FBMAN
            .lock()
            .as_mut()
            .expect("screen uninitialized")
            .write_fmt(args)
            .expect("Printing to screen failed");
    });
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::draw::_print(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($fmt:expr) => ($crate::draw::_print(format_args_nl!($fmt)));
    ($fmt:expr, $($arg:tt)*) => ($crate::draw::_print(
        format_args_nl!($fmt, $($arg)*)
    ));
}

pub struct FrameBufferManager {
    fb: &'static mut [u8],
    pub chars: Box<[char]>,
    pub horiz_chars: usize,
    pub bytes_per_pixel: usize,
    pub stride: usize,
    scale_factor: usize,
    idx: usize,
    /// Shift of the red mask in RGB.
    pub red_mask_shift: u8,
    /// Shift of the green mask in RGB.
    pub green_mask_shift: u8,
    /// Shift of the blue mask in RGB.
    pub blue_mask_shift: u8,
}

impl fmt::Debug for FrameBufferManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameBufferManager")
            .field("horiz_chars", &self.horiz_chars)
            .field("bytes_per_pixel", &self.bytes_per_pixel)
            .field("stride", &self.stride)
            .field("idx", &self.idx)
            .field("red_mask_shift", &self.red_mask_shift)
            .field("green_mask_shift", &self.green_mask_shift)
            .field("blue_mask_shift", &self.blue_mask_shift)
            .finish()
    }
}

impl FrameBufferManager {
    pub fn new(b: &Framebuffer<'_>) -> Self {
        let scale_factor = 8;
        let horiz_res = b.width() as usize;
        let horiz_chars = horiz_res / FONT_WIDTH as usize / scale_factor;

        let vert_res = b.height() as usize;
        let vert_chars = vert_res / FONT_HEIGHT as usize / scale_factor;

        let chars = vec![' '; horiz_chars * vert_chars].into_boxed_slice();

        let bytes_per_pixel = (b.bpp() / 8) as usize;
        let stride = b.pitch() as usize;

        let fb = unsafe { slice::from_raw_parts_mut(b.addr(), b.height() as usize * stride) };

        Self {
            fb,
            chars,
            horiz_chars,
            bytes_per_pixel,
            stride,
            scale_factor,
            idx: 0,
            red_mask_shift: b.red_mask_shift(),
            green_mask_shift: b.green_mask_shift(),
            blue_mask_shift: b.blue_mask_shift(),
        }
    }

    pub fn put(&mut self, c: char) {
        if c == '\n' {
            self.idx = 0;
            self.newline();
            self.redraw();
            return;
        }

        let last_line = self.chars.len() - self.horiz_chars;

        if self.idx == self.horiz_chars {
            // content wraps to the next line
            self.idx = 0;
            self.newline();
            let offset = last_line;
            self.chars[offset] = c;
            self.redraw()
        } else {
            let offset = last_line + self.idx;
            self.chars[offset] = c;
            self.putchar(
                c,
                self.idx,
                self.chars.len() / self.horiz_chars - 1,
                0xA8A8A8,
                0,
            );
        }

        self.idx += 1;
    }

    /// Redraw the whole grid.
    fn redraw(&mut self) {
        let mut x = 0;
        let mut y = 0;
        let horiz_chars = self.horiz_chars;
        for &c in self.chars.as_ref() {
            Self::putc(
                self.fb,
                self.bytes_per_pixel,
                self.stride,
                self.scale_factor,
                c,
                x,
                y,
                0xA8A8A8,
                0,
            );
            if x + 1 == horiz_chars {
                y += 1;
                x = 0;
            } else {
                x += 1;
            }
        }
    }

    #[inline]
    fn newline(&mut self) {
        self.chars.rotate_left(self.horiz_chars);
        let len = self.chars.len();
        self.chars[len - self.horiz_chars..].fill(' ');
    }

    fn putc(
        fb: &mut [u8],
        bytes_per_pixel: usize,
        stride: usize,
        scale_factor: usize,
        c: char,
        cx: usize,
        cy: usize,
        fg: u32,
        bg: u32,
    ) {
        assert_eq!(4, bytes_per_pixel);

        let font_height = FONT_HEIGHT * scale_factor;
        let font_width = FONT_WIDTH * scale_factor;

        let glyph = to_bitmap(c);

        let mut offset = (cy * font_height * stride) + (cx * font_width * bytes_per_pixel);

        for row in glyph {
            for _ in 0..scale_factor {
                let mut line = offset;
                let mut mask = 1 << (FONT_WIDTH - 1);

                for x in 0..font_width {
                    unsafe {
                        let pixel = fb.as_mut_ptr().add(line) as *mut u32;
                        pixel.write_volatile(if row & mask != 0 { fg } else { bg });
                    }
                    if (x + 1) % scale_factor == 0 {
                        mask >>= 1;
                    }
                    line += bytes_per_pixel;
                }
                offset += stride;
            }
        }
    }

    pub fn putchar(&mut self, c: char, cx: usize, cy: usize, fg: u32, bg: u32) {
        Self::putc(
            self.fb,
            self.bytes_per_pixel,
            self.stride,
            self.scale_factor,
            c,
            cx,
            cy,
            fg,
            bg,
        )
    }
}

impl core::fmt::Write for FrameBufferManager {
    fn write_char(&mut self, c: char) -> core::fmt::Result {
        self.put(c);
        Ok(())
    }
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            self.put(c)
        }
        Ok(())
    }
}
