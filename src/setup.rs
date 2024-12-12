use limine::BaseRevision;
use limine::paging::Mode;
use limine::request::{
    FramebufferRequest, HhdmRequest, MemoryMapRequest, PagingModeRequest, RequestsEndMarker,
    RequestsStartMarker, RsdpRequest, StackSizeRequest,
};
use x86_64::VirtAddr;

use crate::time::Mapper;
use crate::time;

pub fn init() {
    let physical_memory_offset = HHDM_REQUEST.get_response().unwrap().offset();
    unsafe {
        crate::mem::init(
            VirtAddr::new(physical_memory_offset),
            MEMORY_MAP_REQUEST.get_response().unwrap().entries(),
        )
    };
    crate::interrupts::init();
    let physical_memory_offset = physical_memory_offset as usize;
    let mapper = Mapper::new(physical_memory_offset);
    let tables = time::get_acpi_tables(
        RSDP_REQUEST.get_response().unwrap().address() as usize - physical_memory_offset,
        mapper,
    );
    let platform_info = time::get_platform_info(&tables);
    time::init_and_disable_old_pic();
    time::init_lapic(&platform_info, &mapper);
    let (ioapic, pitreg) = time::init_ioapic(&platform_info, &mapper);
    time::init(ioapic, pitreg);
    x86_64::instructions::interrupts::enable();

    let frame_buffer = FRAMEBUFFER_REQUEST
        .get_response()
        .unwrap()
        .framebuffers()
        .next()
        .unwrap();

    crate::draw::init(crate::draw::FrameBufferManager::new(&frame_buffer));
}

// 32 KiB of stack
const STACK_SIZE: u64 = 32 * 1024;

/// Sets the base revision to the latest revision supported by the crate.
/// See specification for further info.
/// Be sure to mark all limine requests with #[used], otherwise they may be removed by the compiler.
#[used]
// The .requests section allows limine to find the requests faster and more safely.
#[link_section = ".requests"]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[link_section = ".requests"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[link_section = ".requests"]
static PAGING_MODE_REQUEST: PagingModeRequest =
    PagingModeRequest::new().with_mode(Mode::FIVE_LEVEL);

#[used]
#[link_section = ".requests"]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(STACK_SIZE);

#[used]
#[link_section = ".requests"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[link_section = ".requests"]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[link_section = ".requests"]
static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

/// Define the stand and end markers for Limine requests.
#[used]
#[link_section = ".requests_start_marker"]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();
#[used]
#[link_section = ".requests_end_marker"]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[panic_handler]
#[cfg(not(test))]
pub fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    crate::sprintln!("{}", info);
    crate::utils::hlt_loop()
}
