#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]
#![allow(stable_features, unknown_lints, async_fn_in_trait)]

use defmt::*;
use embassy_executor;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::usart::{self, Uart};
use embassy_stm32::{bind_interrupts, peripherals};
use embassy_time::Timer;
use module_runtime::*;

bind_interrupts!(struct Irqs{
    LPUART1 => usart::InterruptHandler<peripherals::LPUART1>;
});

struct HostUartContext {
    uart: Uart<'static, peripherals::LPUART1, peripherals::DMA1_CH3, peripherals::DMA1_CH4>,
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = init();
    info!("Hello World!");

    let mut led = Output::new(p.PC13, Level::High, Speed::Low);

    let mut lpuart1_config = usart::Config::default();
    lpuart1_config.baudrate = 115200;
    let lpuart1 = Uart::new(
        p.LPUART1,
        p.PA3,
        p.PA2,
        Irqs,
        p.DMA1_CH3,
        p.DMA1_CH4,
        lpuart1_config,
    )
    .unwrap();

    spawner
        .spawn(host_uart_task(HostUartContext { uart: lpuart1 }))
        .unwrap();

    loop {
        info!("high");
        led.set_high();
        Timer::after_millis(500).await;

        info!("low");
        led.set_low();
        Timer::after_millis(500).await;
    }
}

#[embassy_executor::task]
async fn host_uart_task(ctx: HostUartContext) {
    let mut uart = ctx.uart;
    let mut rx_buf = [0u8; 100];

    unwrap!(uart.write(b"test\r\n").await);
    loop {
        let result = uart.read_until_idle(&mut rx_buf).await;
        match result {
            Ok(size) => {
                info!("size {}", size);
                if size > 0 {
                    match uart.write(&rx_buf[0..size]).await {
                        Ok(()) => {}
                        Err(e) => {
                            error!("tx error: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                error!("rx error: {}", e);
            }
        }
    }
}
