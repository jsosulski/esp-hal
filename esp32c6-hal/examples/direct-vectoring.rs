#![no_main]
#![no_std]
#![feature(naked_functions)]
use core::{arch::asm, cell::RefCell};

use critical_section::Mutex;
use esp32c6_hal::{
    clock::ClockControl,
    interrupt::{
        CpuInterrupt,
        {self},
    },
    peripherals::{self, Peripherals},
    prelude::*,
    system::{SoftwareInterrupt, SoftwareInterruptControl},
    timer::TimerGroup,
    Rtc,
};
use esp_backtrace as _;

static SWINT: Mutex<RefCell<Option<SoftwareInterruptControl>>> = Mutex::new(RefCell::new(None));

#[entry]
fn main() -> ! {
    let peripherals = Peripherals::take();
    let mut system = peripherals.PCR.split();
    let clockctrl = system.clock_control;
    let sw_int = system.software_interrupt_control;
    let clocks = ClockControl::boot_defaults(clockctrl).freeze();

    // Disable the watchdog timers. For the ESP32-C6, this includes the Super WDT,
    // the RTC WDT, and the TIMG WDTs.
    let mut rtc = Rtc::new(peripherals.LP_CLKRST);
    let timer_group0 = TimerGroup::new(
        peripherals.TIMG0,
        &clocks,
        &mut system.peripheral_clock_control,
    );
    let mut wdt0 = timer_group0.wdt;
    let timer_group1 = TimerGroup::new(
        peripherals.TIMG1,
        &clocks,
        &mut system.peripheral_clock_control,
    );
    let mut wdt1 = timer_group1.wdt;

    rtc.swd.disable();
    rtc.rwdt.disable();
    wdt0.disable();
    wdt1.disable();

    critical_section::with(|cs| SWINT.borrow_ref_mut(cs).replace(sw_int));
    unsafe {
        interrupt::enable(
            peripherals::Interrupt::FROM_CPU_INTR0,
            interrupt::Priority::Priority3,
            CpuInterrupt::Interrupt1,
        )
        .unwrap();
        asm!(
            "
        csrrwi x0, 0x7e0, 1 #what to count, for cycles write 1 for instructions write 2
        csrrwi x0, 0x7e1, 0 #disable counter
        csrrwi x0, 0x7e2, 0 #reset counter
        "
        );
    }
    esp_println::println!("MPC:{}", unsafe { fetch_performance_timer() });
    // interrupt is raised from assembly for max timer granularity.
    unsafe {
        asm!(
            "
        li t0, 0x600C5090 #FROM_CPU_INTR0 address
        li t1, 1    #Flip flag
        csrrwi x0, 0x7e1, 1 #enable timer
        sw t1, 0(t0) #trigger FROM_CPU_INTR0
        "
        )
    }
    esp_println::println!("Returned");
    loop {}
}

#[no_mangle]
fn cpu_int_1_handler() {
    unsafe { asm!("csrrwi x0, 0x7e1, 0 #disable timer") }
    critical_section::with(|cs| {
        SWINT
            .borrow_ref_mut(cs)
            .as_mut()
            .unwrap()
            .reset(SoftwareInterrupt::SoftwareInterrupt0);
    });
    esp_println::println!("Performance counter:{}", unsafe {
        fetch_performance_timer()
    });
}
#[naked]
unsafe extern "C" fn fetch_performance_timer() -> i32 {
    asm!(
        "
    csrr a0, 0x7e2
    jr ra
    ",
        options(noreturn)
    );
}
