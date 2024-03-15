# STM32WLE LoRa communication module runtime and application firmware

[![Application builds](https://github.com/manakjiri/lora-module-fw/actions/workflows/rust.yml/badge.svg)](https://github.com/manakjiri/lora-module-fw/actions/workflows/rust.yml)

## Dev

Flash node:

- module-bootloader: `DEFMT_LOG=info cargo flash --release --probe 0483:3748 --chip STM32WLE5JCIx`
- module-node: `DEFMT_LOG=info cargo run --release -- --probe 0483:3748 --no-location`

Flash Gateway:

- module-gateway: `DEFMT_LOG=info cargo run --release -- --probe 0483:374e --no-location`
