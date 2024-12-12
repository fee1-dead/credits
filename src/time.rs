//! ACPI, LAPIC timer, etc.
use alloc::alloc::Global;
use core::num::Wrapping;
use core::ptr::NonNull;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering::Relaxed;
use core::time::Duration;

use acpi::{AcpiTables, InterruptModel, PlatformInfo};
use pic8259::ChainedPics;
use x86_64::instructions::port::Port;
use x86_64::instructions::{hlt, interrupts};

use super::interrupts::{InterruptIndex, PIC_1_OFFSET, PIC_2_OFFSET};
use crate::sprintln;

#[derive(Clone, Copy)]
pub struct Mapper {
    pub physical_memory_offset: usize,
}

impl Mapper {
    #[inline]
    pub fn new(physical_memory_offset: usize) -> Self {
        Self {
            physical_memory_offset,
        }
    }

    #[inline]
    pub fn phys_to_virt(&self, phys: usize) -> usize {
        self.physical_memory_offset + phys
    }

    #[inline]
    pub fn phys_to_virt_ptr(&self, phys: usize) -> NonNull<u8> {
        NonNull::new(self.phys_to_virt(phys) as *mut u8).unwrap()
    }
}

impl acpi::AcpiHandler for Mapper {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> acpi::PhysicalMapping<Self, T> {
        acpi::PhysicalMapping::new(
            physical_address,
            NonNull::new_unchecked((self.physical_memory_offset + physical_address) as *mut _),
            size,
            size,
            *self,
        )
    }

    // No-op since we don't remove entries.
    fn unmap_physical_region<T>(_: &acpi::PhysicalMapping<Self, T>) {}
}

pub type Tables = AcpiTables<Mapper>;

pub fn get_acpi_tables(rsdp_addr: usize, mapper: Mapper) -> Tables {
    unsafe { acpi::AcpiTables::from_rsdp(mapper, rsdp_addr) }.expect("ACPI tables")
}

pub fn get_platform_info(tables: &Tables) -> PlatformInfo<'_, Global> {
    let info = PlatformInfo::new(tables).expect("platform info");
    info
}

static mut LAPIC: Option<Lapic> = None;

/// TODO avoid concurrent read/writes
pub fn lapic() -> Lapic {
    unsafe { LAPIC.unwrap_unchecked() }
}

macro_rules! common_apic_methods {
    ($offset:ident) => {
        #[inline]
        pub unsafe fn read_register(&mut self, offset: $offset) -> u32 {
            self.register_at(offset).read_volatile()
        }

        #[inline]
        pub unsafe fn write_register(&mut self, offset: $offset, value: u32) {
            self.register_at(offset).write_volatile(value);
        }

        #[inline]
        pub unsafe fn update_register<F>(&mut self, offset: $offset, f: F)
        where
            F: FnOnce(u32) -> u32,
        {
            let reg = self.read_register(offset);
            self.write_register(offset, f(reg));
        }
    };
}
pub const APIC_TIMER_PERIODIC: u32 = 0x20000;
pub const APIC_MASKED: u32 = 0x10000;

////////////////////////////////////
// REGISTERS

/// The local vector table for LAPIC timer.
///
/// See LVT format at https://wiki.osdev.org/APIC#Local_Vector_Table_Registers
pub const LAPIC_LVT_TIMER_REG: usize = 0x320;

pub const LAPIC_LVT_LINT0_REG: usize = 0x350;

pub const LAPIC_LVT_LINT1_REG: usize = 0x360;

/// The initial count of the timer.
pub const LAPIC_TIMER_INITCNT_REG: usize = 0x380;

/// The current count of the timer.
pub const LAPIC_TIMER_CURRCNT_REG: usize = 0x390;

/// The divider of the timer.
pub const LAPIC_TIMER_DIV_REG: usize = 0x3E0;

/// Local APIC.
#[derive(Clone, Copy)]
pub struct Lapic {
    /// pointer (virtual memory) to the start address of this APIC.
    pub start_ptr: NonNull<u8>,
}

impl Lapic {
    #[inline]
    pub unsafe fn end_of_interrupt(&mut self) {
        self.write_register(0xB0, 0);
    }

    pub unsafe fn register_at(&mut self, offset: usize) -> *mut u32 {
        self.start_ptr.as_ptr().add(offset).cast()
    }

    common_apic_methods!(usize);
}

/// I/O APIC.
#[derive(Clone, Copy)]
pub struct IoApic {
    /// virtual memory pointer to IOREGSEL.
    pub start_ptr: NonNull<u8>,
}

impl IoApic {
    pub unsafe fn register_at(&mut self, offset: u8) -> *mut u32 {
        // tell IOREGSEL where we want to write to
        self.start_ptr
            .as_ptr()
            .cast::<u32>()
            .write_volatile(offset as _);

        self.start_ptr.as_ptr().add(0x10).cast()
    }

    common_apic_methods!(u8);
}

pub const IOAPICVER: u8 = 1;

/// Initialize and disable the old PIC.
pub fn init_and_disable_old_pic() {
    let mut chained_pics = unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) };

    unsafe {
        chained_pics.initialize();
        chained_pics.disable();
    }
}

pub fn init_lapic(platform_info: &PlatformInfo<'_, Global>, mapper: &Mapper) {
    let apic = match &platform_info.interrupt_model {
        InterruptModel::Apic(apic) => apic,
        _ => panic!("unknown interrupt model"),
    };

    let lapic_addr = apic.local_apic_address;

    let start_ptr = mapper.phys_to_virt_ptr(lapic_addr as usize);
    let mut lapic = Lapic { start_ptr };

    // Set the Spurious Interrupt Vector Register bit 8 to start receiving interrupts.
    unsafe {
        lapic.update_register(0xF0, |reg| reg | 0x100);
    }

    unsafe {
        for reg in [
            LAPIC_LVT_TIMER_REG,
            LAPIC_LVT_LINT0_REG,
            LAPIC_LVT_LINT1_REG,
        ] {
            lapic.write_register(reg, APIC_MASKED);
        }
    }

    unsafe {
        LAPIC = Some(lapic);
    }
}

/// Initialize the I/O APIC to enable PIT interrupts.
///
/// Returns the I/O APIC wit the PIT register.
pub fn init_ioapic(platform_info: &PlatformInfo<'_, Global>, mapper: &Mapper) -> (IoApic, u8) {
    let apic = match &platform_info.interrupt_model {
        InterruptModel::Apic(apic) => apic,
        _ => panic!("unknown interrupt model"),
    };

    // find IRQ0 which is emitted by the PIT.
    let pit = apic
        .interrupt_source_overrides
        .iter()
        .find(|ov| ov.isa_source == 0x0)
        .expect("no PIT");
    let overriden_index = pit.global_system_interrupt;

    // find the I/O APIC that handles IRQ0.
    let mut found_reg = None;
    for io_apic in apic.io_apics.iter() {
        let address = io_apic.address;
        let base = io_apic.global_system_interrupt_base;

        let mut ioapic = IoApic {
            start_ptr: mapper.phys_to_virt_ptr(address as usize),
        };

        let ioapicver = unsafe { ioapic.read_register(IOAPICVER) };

        // https://wiki.osdev.org/IOAPIC#IOAPICVER
        let max_redir_count = (ioapicver >> 16) as u8 + 1;

        let mut bsp_acpi_id = platform_info
            .processor_info
            .as_ref()
            .expect("apic proc info")
            .boot_processor
            .local_apic_id;
        // the correct place for destination.
        bsp_acpi_id <<= 56 - 32;

        for idx in 0..max_redir_count {
            // https://wiki.osdev.org/IOAPIC#IOREDTBL
            let reg = 0x10 + idx * 2;
            if idx as u32 + base == overriden_index {
                let vector = InterruptIndex::Timer.as_u8() as u32;

                // The default flags for the register are all zeros. The only interesting bits are the vector bits.
                unsafe {
                    ioapic.write_register(reg, vector);
                }

                // set the processor to send interrupts to. In this case it is the bootstrap processor.
                unsafe {
                    ioapic.write_register(reg + 1, bsp_acpi_id);
                }

                found_reg = Some((ioapic, reg));
            } else {
                // Per https://wiki.osdev.org/APIC#IO_APIC_Registers, set the "masked" flag
                // of other redir entries
                unsafe { ioapic.update_register(reg, |v| v | 1 << 16) }
            }
        }
    }

    found_reg.expect("could not find redirected PIT IRQ")
}

/// number of APIC ticks in 10ms, used by AP init sequence.
///
/// Note that this is NOT the number of IRQs per 10ms.
static APIC_TICKS_IN_10MS: AtomicU32 = AtomicU32::new(0);

// hey guys check out this super unsafe code I wrote
// should be fine though, we're only using a single core
pub static mut TIMER: Wrapping<usize> = Wrapping(0);

fn get_irq_cnt() -> Wrapping<usize> {
    unsafe { (&raw mut TIMER).read_volatile() }
}

/// Configure the programmable interval timer for transition to
/// the Local APIC timer. Interrupts must not be enabled.
fn calibrate_apic_timer(mut ioapic: IoApic, pitreg: u8) {
    // set a divider for 100Hz which is 10ms per IRQ from the PIT.
    let divider = 11932u16;

    // configure the PIT to send an IRQ every 10ms.
    let mut channel0 = Port::new(0x40);
    unsafe {
        // select channel 0, access mode lobyte/hibyte, mode 2 rate generator
        Port::new(0x43).write(0b00110100u8);

        // send the lo/hi bytes to set the reload value.
        channel0.write(divider as u8);
        channel0.write((divider >> 8) as u8);
    }

    let mut lapic = lapic();

    // prepare LAPIC timer
    unsafe {
        // there are other flags the lvt register allows configuring.
        // for now set the interrrupt vector, all other flags are irrelevant.
        //
        // the timer will be in one-shot mode, meaning it will start decrementing
        // the count value when we set an init count.
        lapic.write_register(LAPIC_LVT_TIMER_REG, InterruptIndex::ScratchTimer as u32);
        // set the divide value to 16.
        lapic.write_register(LAPIC_TIMER_DIV_REG, 3);
    }

    // enable interrupts
    interrupts::enable();

    // we need to wait until PIT interrupts so the delay is as accurate as possible
    let saved_pit_cnt = get_irq_cnt();
    let mut curr_pit_cnt;

    loop {
        curr_pit_cnt = get_irq_cnt();

        if curr_pit_cnt - saved_pit_cnt >= Wrapping(1) {
            break;
        }
    }

    // PIT just emitted IRQ, start LAPIC timer.
    unsafe {
        lapic.write_register(LAPIC_TIMER_INITCNT_REG, u32::MAX);
    }

    // wait for another IRQ from the PIT.
    while get_irq_cnt() - curr_pit_cnt < Wrapping(1) {}

    // Stop the APIC timer
    unsafe {
        lapic.write_register(LAPIC_LVT_TIMER_REG, APIC_MASKED);
    }

    // we've now measured the number of LAPIC ticks in 10ms.
    let apic_ticks_in_10ms = u32::MAX - unsafe { lapic.read_register(LAPIC_TIMER_CURRCNT_REG) };

    interrupts::disable();

    sprintln!("apic ticks in 10ms = {apic_ticks_in_10ms}");

    APIC_TICKS_IN_10MS.store(apic_ticks_in_10ms, Relaxed);

    // mask the PIT I/O APIC entry.
    unsafe { ioapic.write_register(pitreg, APIC_MASKED) }

    // configure the lapic timer to send an IRQ per 10ms periodically.
    unsafe {
        // use the `Timer` IRQ instead of `ScratchTimer`. Enable periodic mode.
        lapic.write_register(
            LAPIC_LVT_TIMER_REG,
            InterruptIndex::Timer as u32 | APIC_TIMER_PERIODIC,
        );
        lapic.write_register(LAPIC_TIMER_INITCNT_REG, apic_ticks_in_10ms);
        // set the divide value to 16 again. This is not required by the manuals.
        // but according to OSDev wiki there are buggy hardware out there that needs this.
        lapic.write_register(LAPIC_TIMER_DIV_REG, 3);
    }
}

/// Configure to have 100 Timer IRQs per second, i.e. 1 IRQ per 10ms.
///
/// Interrupts should not be enabled but should be properly configured
/// before calling this. Interrupts will not be enabled when this function
/// returns.
///
/// The programmable interval timer (PIT) should be configured to IRQ at
/// `InterruptIndex::Timer`. We currently use it to calibrate the APIC timer.
pub fn init(ioapic: IoApic, pitreg: u8) {
    sprintln!("im currently here1");
    calibrate_apic_timer(ioapic, pitreg);
    sprintln!("im currently here2");
}

/// precision microsecond delay, `micros` should not be larger than 1000.
pub fn udelay(micros: usize) {
    // instead of using the IRQ counter, we need to read the current count
    // register directly.
    let ticks_per_10ms = APIC_TICKS_IN_10MS.load(Relaxed) as usize;
    let delay_ticks = ticks_per_10ms * micros / 10000;

    let get_tick = || unsafe { lapic().read_register(LAPIC_TIMER_CURRCNT_REG) } as usize;

    let current_tick = get_tick();

    // not hlt-ing here, as the duration is smaller than 1ms
    //
    // N.B. the orders are swapped here because the LAPIC timer decrements
    // the tick counter while our kernel IRQ handler increments the counter.
    while current_tick - get_tick() < delay_ticks {}
}

/// precision millis delay
fn mdelay(mut millis: usize) {
    while millis >= 10 {
        let curr_irq_cnt = get_irq_cnt();

        while get_irq_cnt() - curr_irq_cnt < Wrapping(1) {
            hlt();
        }

        millis -= 10;
    }
}

/// Simple spin delay
pub fn delay(dur: Duration) {
    if dur.as_micros() < 1000 {
        udelay(dur.as_micros() as usize);
    } else {
        mdelay(
            dur.as_millis()
                .try_into()
                .expect("delay duration is too long"),
        );
    }
}
