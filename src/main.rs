use anyhow::Result;
use embedded_hal::i2c::I2c;
use core::str;
use embedded_svc::{
    http::{Headers, Method},
    io::Write,
};
use esp_idf_hal::{gpio::{AnyOutputPin, Output, OutputPin, PinDriver}, io::Read};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        i2c::{I2cConfig, I2cDriver},
        io::EspIOError,
        prelude::*,
    },
    http::server::{Configuration, EspHttpServer},
};
use log::info;
use rgb_led::{RGB8, WS2812RMT};
use serde::Deserialize;
use std::{
    sync::{Arc, Mutex},
    thread::sleep,
    time::Duration,
};
use wifi::wifi;

#[toml_cfg::toml_config]
pub struct Config {
    #[default("")]
    wifi_ssid: &'static str,
    #[default("")]
    wifi_psk: &'static str,
}

#[derive(Deserialize)]
struct FormData<'a> {
    fm_frequency: f32,
    web_station: &'a str,
}

const MAX_CONTROL_PAYLOAD_LEN: usize = 128;
static CONTROL_RADIO_HTML: &str = include_str!("control-radio.html");

const SI4703_I2C_ADDR: u8 = 0x10;

struct Si4703<I2C> {
    i2c: I2C,
    // sen: GpioPin<Output>,
    // rst: GpioPin<Output>,
    // gpio1: GpioPin<Output>,
    // gpio2: GpioPin<Output>,
}

impl<I2C, E> Si4703<I2C>
where
    I2C: I2c<Error = E>,
{
    pub fn new(i2c: I2C, /*sen: PinDriver<OutputPin, Output>, rst: AnyOutputPin<Output>, gpio1: GpioPin<Output>, gpio2: GpioPin<Output>*/) -> Result<Self, E> {
        //let sen_pin = PinDriver::output(sen);
        let mut radio = Si4703 { i2c};//, sen: sen_pin, rst, gpio1, gpio2 };

        // Initialize control pins
        // radio.sen.set_high()?;
        // radio.gpio1.set_high()?;
        // radio.gpio2.set_high()?;

        // Perform necessary initialization
        // Example: reset command
        let init_command = [0x02];
        radio.i2c.write(SI4703_I2C_ADDR, &init_command)?;

        // Reset the SI4703
        radio.reset()?;

        // Delay to allow the device to reset
        sleep(Duration::from_millis(100));

        Ok(radio)
    }

    pub fn reset(&mut self) -> Result<(), E> {
        // Toggle the RST pin to reset the device
        // self.rst.set_low()?;
        sleep(Duration::from_millis(10));
        // self.rst.set_high()?;
        sleep(Duration::from_millis(10));

        Ok(())
    }

    pub fn set_volume(&mut self, volume: u8) -> Result<(), E> {
        // Volume should be between 0 and 15
        assert!(volume <= 15);

        // Example command to set volume
        let volume_command = [0x05, volume << 4];
        self.i2c.write(SI4703_I2C_ADDR, &volume_command)?;

        Ok(())
    }

    pub fn tune(&mut self, frequency: u16) -> Result<(), E> {
        // Frequency in 10 kHz steps (e.g., 1011 for 101.1 MHz)
        let frequency_command = [
            0x03,
            (frequency >> 8) as u8,
            (frequency & 0xFF) as u8,
        ];
        self.i2c.write(SI4703_I2C_ADDR, &frequency_command)?;

        Ok(())
    }
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let sysloop = EspSystemEventLoop::take()?;

    let app_config = CONFIG;

    let _wifi = wifi(
        app_config.wifi_ssid,
        app_config.wifi_psk,
        peripherals.modem,
        sysloop,
    )?;
    info!("Pre led");

    // Wrap the led in an Arc<Mutex<...>>
    let led = Arc::new(Mutex::new(WS2812RMT::new(
        peripherals.pins.gpio8,
        peripherals.rmt.channel0,
    )?));
    {
        let mut led = led.lock().unwrap();
        led.set_pixel(RGB8::new(50, 0, 0))?;
    }
    info!("Post led");

    // Initialize radio tuner
    let sda = peripherals.pins.gpio6;
    let scl = peripherals.pins.gpio7;
    let sen = peripherals.pins.gpio0;
    let rst = peripherals.pins.gpio1;
    let gpio1 = peripherals.pins.gpio10;
    let gpio2 = peripherals.pins.gpio11;
    let config = I2cConfig::new().baudrate(400.kHz().into());
    println!("config ok");
    let i2c = I2cDriver::new(peripherals.i2c0, sda, scl, &config)?;
    println!("i2c ok");

    let mut si4703 = Si4703::new(i2c)?;//, sen, rst, gpio1, gpio2)?;
    println!("si4703 created");

    // Set volume to max
    si4703.set_volume(15)?;
    println!("Volume set to 15");

    // Tune to a frequency (e.g., 103.9 MHz)
    si4703.tune(1039)?;

    println!("Tuned to 103.9 MHz");

    // let radio_tuner = Arc::new(Mutex::new(
    //     TEA5767::new(i2c, 103.9, BandLimits::EuropeUS, SoundMode::Stereo).unwrap(),
    // ));
    // let mut radio = Si4703::new(i2c);
    // radio.enable_oscillator().map_err(|e| format!("Enable oscillator error: {:?}", e));
    // sleep(Duration::from_millis(500));
    // radio.enable().map_err(|e| format!("Enable error: {:?}", e));
    // sleep(Duration::from_millis(110));

    // radio.set_volume(Volume::Dbfsm28).map_err(|e| format!("Volume error: {:?}", e));
    // radio.set_deemphasis(DeEmphasis::Us50).map_err(|e| format!("Deemphasis error: {:?}", e));
    // radio.set_channel_spacing(ChannelSpacing::Khz100).map_err(|e| format!("Channel spacing error: {:?}", e));
    // radio.unmute().map_err(|e: si4703::Error<esp_idf_hal::i2c::I2cError>| format!("Unmute error: {:?}", e));
    

    let mut server = EspHttpServer::new(&Configuration::default())?;

    // Clone the Arc to pass to the closure
    let led_clone = led.clone();
    server.fn_handler(
        "/",
        Method::Get,
        move |request| -> core::result::Result<(), EspIOError> {
            let html = index_html();
            let mut response = request.into_ok_response()?;
            response.write_all(html.as_bytes())?;
            let mut led = led_clone.lock().unwrap();
            let _ = led.set_pixel(RGB8::new(0, 50, 0));
            Ok(())
        },
    )?;

    server.fn_handler("/radio", Method::Get, |req| {
        req.into_ok_response()?
            .write_all(CONTROL_RADIO_HTML.as_bytes())
            .map(|_| ())
    })?;

    let led_clone = led.clone();
    // let radio_tuner_clone = radio_tuner.clone();
    server.fn_handler::<anyhow::Error, _>("/post-radio-form", Method::Post, move |mut req| {
        let len = req.content_len().unwrap_or(0) as usize;

        if len > MAX_CONTROL_PAYLOAD_LEN {
            req.into_status_response(413)?
                .write_all("Request too big".as_bytes())?;
            return Ok(());
        }

        let mut buf = vec![0; len];
        req.read_exact(&mut buf)?;
        let mut resp = req.into_ok_response()?;

        if let Ok(form) = serde_json::from_slice::<FormData>(&buf) {
            // let mut radio_tuner = radio_tuner_clone
            //     .lock()
            //     .map_err(|_| anyhow::anyhow!("Failed to lock radio tuner mutex"))?;
            // radio_tuner
            //     .set_frequency(form.fm_frequency)
            //     .map_err(|_| anyhow::anyhow!("Failed to set radio tuner frequency"))?;
            let mut led = led_clone.lock().unwrap();
            let _ = led.set_pixel(RGB8::new(0, 0, 0));
            sleep(Duration::from_millis(100));
            let _ = led.set_pixel(RGB8::new(0, 50, 0));
            write!(
                resp,
                "Requested {} FM and {} station",
                form.fm_frequency, form.web_station
            )?;
        } else {
            resp.write_all("JSON error".as_bytes())?;
        }

        Ok(())
    })?;

    // radio_tuner.set_frequency(fm_frequency).unwrap();
    // let _ = radio_tuner.mute();
    // radio_tuner.set_standby();
    // radio_tuner.reset_standby();

    println!("Server awaiting connection");

    loop {
        info!("tick");
        sleep(Duration::from_millis(1000));
    }
}

fn templated(content: impl AsRef<str>) -> String {
    format!(
        r#"
<!DOCTYPE html>
<html>
    <head>
        <meta charset="utf-8">
        <title>ISS web server</title>
    </head>
    <body>
        {}
    </body>
</html>
"#,
        content.as_ref()
    )
}

fn index_html() -> String {
    templated("Hello from ISS!")
}

// fn fm_frequency_page(val: f32) -> String {
//     templated(format!("Current FM frequency is: {:.2}", val))
// }
