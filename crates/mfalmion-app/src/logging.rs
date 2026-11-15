//! Small stderr logger; diagnostics are opt-in and never write runtime files.
struct Logger;
static LOGGER: Logger = Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
            && (metadata.level() <= log::Level::Warn
                || metadata.target().starts_with("eframe")
                || metadata.target().starts_with("egui_wgpu")
                || metadata.target().starts_with("wgpu")
                || metadata.target().starts_with("winit"))
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "level={} target={} message={:?}",
                record.level(),
                record.target(),
                record.args().to_string()
            );
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(if std::env::var_os("MFALMION_DIAGNOSTICS").is_some() {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Warn
        });
    }
}
