#![no_std]
#![no_main]
#![allow(unused_imports)]

use embassy_executor::Spawner;
use embedded_hal::digital::OutputPin;

use core::cell::RefCell;
use core::panic::PanicInfo;

use embassy_embedded_hal::shared_bus::blocking::spi::SpiDeviceWithConfig;
use embassy_rp::gpio::{Level, Output, Input};
use embassy_rp::{bind_interrupts, spi};
// USB driver
use embassy_rp::peripherals::USB;
use embassy_rp::spi::{Blocking, Spi};
use embassy_rp::usb::{Driver, InterruptHandler as USBInterruptHandler};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{Delay, Timer};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::image::{Image, ImageRawLE};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::text::renderer::CharacterStyle;
use embedded_graphics::prelude::*;
use embedded_graphics::*;
use embedded_graphics::primitives::*;
use heapless::Vec;
use log::info;
use st7789::{Orientation, ST7789};

mod display;

use display::SPIDeviceInterface;

use embassy_rp::adc::{Adc, Channel, Config as AdcConfig, InterruptHandler as AdcInterruptHandler};
use embassy_rp::gpio::Pull;


const DISPLAY_FREQ: u32 = 64_000_000;


bind_interrupts!(struct Irqs {
    // Use for the serial over USB driver
    USBCTRL_IRQ => USBInterruptHandler<USB>;
    ADC_IRQ_FIFO => AdcInterruptHandler;
});

#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USB>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

fn calculate_temperature(temperature_adc: u32) -> i32 {
    let var1: i32 = ((temperature_adc as i32 >> 3) - (27504 << 1)) * (26435 >> 11);
    let var2: i32 = ((temperature_adc as i32 >> 4) - 27504)
        * (((temperature_adc as i32 >> 4) - 27504) >> 12)
        * (-1000 >> 14);
    ((var1 + var2) * 5 + 128) >> 8
}

    fn adc_to_voltage(adc_value: u16) -> f32 {
        (adc_value as f32 / 1023 as f32) * 3.3
    }

    fn voltage_to_soil_moisture(voltage: f32) -> f32 {
        ((10.0 - voltage) / (10.0 - 4.5))*100.0
    }

    fn calculate_light(light_adc: u16) -> f32 {
        let adjusted_adc = if light_adc > 3000 {
            light_adc - 4000
        } else if light_adc < 1000 {
            light_adc + 4000
        } else {
            light_adc
        };
    
        adjusted_adc as f32
    }
    


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let peripherals = embassy_rp::init(Default::default());

    // USB logger driver
    let driver = Driver::new(peripherals.USB, Irqs);
    spawner.spawn(logger_task(driver)).unwrap();

    info!("Raspberry Pi Started");

    let mut adc = Adc::new(peripherals.ADC, Irqs, AdcConfig::default());
    let mut temperature_sensor = Channel::new_pin(peripherals.PIN_27, Pull::None);
    let mut soil_sensor = Channel::new_pin(peripherals.PIN_26, Pull::None);
    let mut light_sensor = Channel::new_pin(peripherals.PIN_28, Pull::None);
    let mut relay = Output::new(peripherals.PIN_0, Level::Low);


    let mut display_config = spi::Config::default();
    display_config.frequency = DISPLAY_FREQ;
    display_config.phase = spi::Phase::CaptureOnSecondTransition;
    display_config.polarity = spi::Polarity::IdleHigh;

    // Display SPI pins
    let miso = peripherals.PIN_8;
    let mosi = peripherals.PIN_11;
    let clk = peripherals.PIN_10;

    // Display SPI on SPI0
    let spi_display: Spi<'_, _, Blocking> =
        Spi::new_blocking(peripherals.SPI1, clk, mosi, miso, display_config.clone());
    // SPI bus for display
    let spi_bus: Mutex<NoopRawMutex, _> = Mutex::new(RefCell::new(spi_display));

    let display_cs = Output::new(peripherals.PIN_9, Level::High);

    // Display SPI device initialization
    let display_spi = SpiDeviceWithConfig::new(&spi_bus, display_cs, display_config);

    // Other display pins
    let rst = peripherals.PIN_12;
    let dc = peripherals.PIN_7;
    let dc = Output::new(dc, Level::Low);
    let rst = Output::new(rst, Level::Low);
    let di = SPIDeviceInterface::new(display_spi, dc);

    // Init ST7789 LCD
    let mut display = ST7789::new(di, rst, 240, 240);
    display.init(&mut Delay).unwrap();
    display.set_orientation(Orientation::LandscapeSwapped).unwrap();
    display.clear(Rgb565::BLACK).unwrap();

    // Define style
    let mut style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_PINK);
    style.set_background_color(Some(Rgb565::BLACK));

    let text = embedded_graphics::text::Text::new("Rust in peace", Point::new(150, 20), style);
    let dry = embedded_graphics::text::Text::new("I need water", Point::new(130, 80), style);
    let wet = embedded_graphics::text::Text::new("I am drowning", Point::new(130, 80), style);
    let dark = embedded_graphics::text::Text::new("I can't see", Point::new(130, 40), style);
    let diamond = embedded_graphics::text::Text::new("Shine bright", Point::new(130, 40), style);
    let frozen = embedded_graphics::text::Text::new("Frozen castle", Point::new(130, 60), style);
    let hot = embedded_graphics::text::Text::new("Too hot", Point::new(130, 60), style);
    let fine = embedded_graphics::text::Text::new("FloraMood", Point::new(150, 20), style);

    let raw_image_data = ImageRawLE::new(include_bytes!("../assets/happy/image_1.raw"), 120);
    let mut ferris = Image::new(&raw_image_data, Point::new(150, 150));

    const IMAGE_WIDTH: u32 = 120;
    const MAX_IMAGES: usize = 100;
    let mut images: Vec<ImageRawLE<Rgb565>, MAX_IMAGES> = Vec::new();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_1.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_2.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_3.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_4.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_5.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_6.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_7.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_8.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_9.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_10.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_11.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_12.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_13.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_14.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_15.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_16.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_17.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_18.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_19.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_20.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_21.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_22.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_23.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_24.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_25.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_26.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_27.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_28.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_29.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_30.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_31.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_32.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_33.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_34.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_35.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_36.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_37.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_38.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_39.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_40.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_41.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_42.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_43.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_44.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_45.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_46.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_47.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_48.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_49.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_50.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_51.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_52.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_53.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_54.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_55.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_56.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_57.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_58.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_59.raw"), IMAGE_WIDTH)).unwrap();
    images.push(ImageRawLE::new(include_bytes!("../assets/happy/image_60.raw"), IMAGE_WIDTH)).unwrap();
    
    let mut i = 0;
    let mut value = 1;

    //////////////////////////////////////////////////////////////////////////////


    loop{
        fine.draw(&mut display).unwrap();
        info!("{}",i);
        ferris = Image::new(&images[i], Point::new(140, 150));
        ferris.draw(&mut display).unwrap();
        
        i = i + 1 ;
        if(i%60==0){i=i/60;}


        let level = adc.read(&mut temperature_sensor).await.unwrap();
        let temperature_value = adc_to_voltage(level) + 7.0;
        info!("Temperature sensor reading: {:.2} °C", temperature_value);
        let level_soil = adc.read(&mut soil_sensor).await.unwrap();
        let level_light = adc.read(&mut light_sensor).await.unwrap();

        // Convert ADC value to voltage
        let soil_value = adc_to_voltage(level_soil);

        // Convert voltage to soil moisture percentage
        let soil_moisture = voltage_to_soil_moisture(soil_value);
        let light_value = calculate_light(level_light);

        if (soil_moisture < 5.0 || soil_moisture > 70.0) && (light_value < 500.0 || light_value > 2000.0) && (temperature_value < 0.0 || temperature_value > 40.0){
            text.draw(&mut display).unwrap();
        }
       
        
        if soil_moisture < 10.0 {
            dry.draw(&mut display).unwrap();
            Timer::after_millis(100).await;
            value = 0;
        }
        if soil_moisture > 10.0 {
            wet.draw(&mut display).unwrap();
            Timer::after_millis(100).await;
            value = 1;
        }
        match value {
            0 => relay.set_low(), 
            _ => relay.set_high(),
        }
        if light_value < 500.0 {
            dark.draw(&mut display).unwrap();
            Timer::after_millis(100).await;
            //relay.set_low();
        }
        if light_value > 2000.0 {
            diamond.draw(&mut display).unwrap();
            Timer::after_millis(100).await;
            //relay.set_high();
        }
        if temperature_value < 0.0 {
            frozen.draw(&mut display).unwrap();
            Timer::after_millis(100).await;
        }
        if temperature_value > 40.0 {
            hot.draw(&mut display).unwrap();
            Timer::after_millis(100).await;
        }
        

        // Log the soil moisture percentage
        info!("Soil Moisture: {:.2}%", soil_moisture);
        info!("Light level: {:.2}", light_value);

        // Small delay for yielding
        Timer::after_millis(16).await;
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
