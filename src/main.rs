use anyhow::Result;
use core::str;
use embedded_svc::{http::Method, io::Write};
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
use std::{
    sync::{Arc, Mutex},
    thread::sleep,
    time::Duration,
};
use tea5767::defs::{BandLimits, SearchAdcLevel, SearchStatus, SoundMode, TEA5767};
use wifi::wifi;

#[toml_cfg::toml_config]
pub struct Config {
    #[default("")]
    wifi_ssid: &'static str,
    #[default("")]
    wifi_psk: &'static str,
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

    // Initialize temperature sensor
    let sda = peripherals.pins.gpio6;
    let scl = peripherals.pins.gpio7;
    let i2c = peripherals.i2c0;
    let config = I2cConfig::new().baudrate(400.kHz().into());
    let i2c = I2cDriver::new(i2c, sda, scl, &config)?;

    let mut radio_tuner =
        TEA5767::new(i2c, 103.9, BandLimits::EuropeUS, SoundMode::Stereo).unwrap();

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

    radio_tuner.set_frequency(104.3).unwrap();
    radio_tuner.mute();
    // radio_tuner.set_standby();
    // radio_tuner.reset_standby();

    // Clone the Arc to pass to the closure
    let led_clone = led.clone();
    server.fn_handler(
        "/temperature",
        Method::Get,
        move |request| -> core::result::Result<(), EspIOError> {
            let temp_val = 12.34;
            let html = temperature(temp_val);
            let mut response = request.into_ok_response()?;
            response.write_all(html.as_bytes())?;
            let mut led = led_clone.lock().unwrap();
            let _ = led.set_pixel(RGB8::new(0, 0, 50));
            Ok(())
        },
    )?;

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
        <title>esp-rs web server</title>
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
    templated("Hello from ESP32-C3!")
}

fn temperature(val: f32) -> String {
    templated(format!("Chip temperature: {:.2}°C", val))
}
