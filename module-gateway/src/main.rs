#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

mod gateway;

use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select::*;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use gateway::*;
use gateway_host_schema::{GatewayPacket, HostPacket};
use module_runtime::*;

static HOST2GATEWAY: Channel<ThreadModeRawMutex, HostPacket, 2> = Channel::new();
static GATEWAY2HOST: Channel<ThreadModeRawMutex, GatewayPacket, 2> = Channel::new();

#[embassy_executor::task]
pub async fn gateway_task(mut lora: ModuleLoRa) {
    let mut gw = Gateway::new();
    loop {
        match select(HOST2GATEWAY.receive(), lora.receive_continuous()).await {
            Either::First(p) => match gw.process_host_message(&mut lora, p).await {
                Ok(resp) => {
                    if let Some(r) = resp {
                        GATEWAY2HOST.send(r).await;
                    }
                }
                Err(e) => {
                    error!("failed to process host message: {}", e);
                }
            },
            Either::Second(lora_result) => match lora_result {
                Ok(p) => {
                    match p.packet_type {
                        LoRaPacketType::OTA => {
                            match gw.process_peer_message(&mut lora, p).await {
                                Ok(resp) => {
                                    if let Some(r) = resp {
                                        GATEWAY2HOST.send(r).await;
                                    }
                                }
                                Err(e) => {
                                    error!("failed to process peer message: {}", e);
                                }
                            }
                        },
                        LoRaPacketType::SoilSensor => {
                            let mut data = [0u16; 4];
                            for i in 0..4 {
                                data[i] = u16::from_le_bytes(p.payload[i*2..i*2+2].try_into().unwrap());
                            }
                            GATEWAY2HOST.send(GatewayPacket::SoilSensorMoisture(data)).await;
                        }
                        _ => {
                            error!("unexpected packet type: {:?}", p.packet_type);
                        }
                    }
                    status_led(LedCommand::FlashShort).await;
                }
                Err(e) => {
                    error!("failed lora receive: {}", e);
                }
            },
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let module = init(ModuleConfig::new(ModuleVersion::NucleoWL55JC), &spawner).await;

    info!("hello from gateway {}", module.lora.address);
    spawner.spawn(gateway_task(module.lora)).unwrap();

    let mut host = module.host;
    let mut uart_buffer = [0u8; 128];
    loop {
        match select(host.read(&mut uart_buffer), GATEWAY2HOST.receive()).await {
            Either::First(uart_result) => {
                match uart_result {
                    Ok(size) => match postcard::from_bytes::<HostPacket>(&uart_buffer[..size]) {
                        Ok(p) => {
                            HOST2GATEWAY.send(p).await;
                        }
                        Err(e) => {
                            error!("failed to parse packet from host: {}", e);
                        }
                    },
                    Err(e) => {
                        error!("uart: {}", e);
                    }
                }
                status_led(LedCommand::FlashShort).await;
            }
            Either::Second(p) => {
                let mut tx_buffer = [0u8; 256];
                match postcard::to_slice(&p, &mut tx_buffer) {
                    Ok(b) => {
                        if let Err(_e) = host.write(&b).await {
                            error!("failed to transmit packet to host");
                        }
                    }
                    Err(e) => {
                        error!("failed to serialize packet: {}", e);
                    }
                }
            }
        }
    }
}
