use embassy_futures::select::*;
use embassy_stm32::{
    exti::{AnyChannel, ExtiInput},
    gpio::{AnyPin, Level, Output, OutputOpenDrain, Pull, Speed, Flex},
};
use embassy_time::{Duration, Instant, Timer};
use module_runtime::*;

pub struct SoilSensor<'a> {
    mux_in1: Output<'a>,
    mux_in2: Output<'a>,
    mux_nen: Output<'a>,
    chg_47k: Flex<'a>,
    chg_4m7: Flex<'a>,
    dischg: OutputOpenDrain<'a>,
    comp: ExtiInput<'a>,
}

#[derive(defmt::Format, Debug, Clone, Copy)]
pub enum SoilSensorResult {
    Timeout,
    Ok(Duration),
}

impl SoilSensor<'_> {
    pub fn new(
        mux_in1: AnyPin,
        mux_in2: AnyPin,
        mux_nen: AnyPin,
        chg_47k: AnyPin,
        chg_4m7: AnyPin,
        dischg: AnyPin,
        comp: AnyPin,
        comp_exti: AnyChannel,
    ) -> Self {
        SoilSensor {
            mux_in1: Output::new(mux_in1, Level::High, Speed::Low),
            mux_in2: Output::new(mux_in2, Level::High, Speed::Low),
            mux_nen: Output::new(mux_nen, Level::High, Speed::Low),
            chg_47k: Flex::new(chg_47k),
            chg_4m7: Flex::new(chg_4m7),
            dischg: OutputOpenDrain::new(dischg, Level::High, Speed::Low, Pull::None),
            comp: ExtiInput::new(comp, comp_exti, Pull::None),
        }
    }

    pub async fn sample_current_channel(&mut self) -> SoilSensorResult {
        self.chg_47k.set_as_output(Speed::Low);
        self.chg_47k.set_low();
        self.chg_4m7.set_as_input(Pull::None);
        /* start discharging */
        self.mux_nen.set_low();
        self.dischg.set_low();
        Timer::after_micros(100).await;
        /* discharge done */
        self.dischg.set_high();
        //info!("comp: {}", comp.is_high());

        /* measure */
        //info!("measuring");
        self.chg_47k.set_high();
        let start = Instant::now();
        let ret = match select(self.comp.wait_for_high(), Timer::after_millis(1)).await {
            Either::First(_) => SoilSensorResult::Ok(start.elapsed()),
            Either::Second(_) => SoilSensorResult::Timeout,
        };
        //info!("comp: {}", comp.is_high());
        self.chg_47k.set_as_input(Pull::None);
        self.dischg.set_low();

        self.mux_nen.set_high();
        ret
    }

    pub fn set_channel(&mut self, channel: u8) {
        self.mux_in1.set_level((channel & 1 == 1).then_some(Level::High).unwrap_or(Level::Low));
        self.mux_in2.set_level((channel & 2 == 2).then_some(Level::High).unwrap_or(Level::Low));
    }

    pub async fn sample_all(&mut self) -> [SoilSensorResult; 4] {
        let mut results = [SoilSensorResult::Timeout; 4];
        for i in 0..4 {
            self.set_channel(i);
            results[i as usize] = self.sample_current_channel().await;
        }
        results
    }
}
