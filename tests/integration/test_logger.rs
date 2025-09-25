use log::{LevelFilter, Log, Metadata, Record};
use std::sync::Once;

static INIT: Once = Once::new();

pub fn init() {
    INIT.call_once(|| {
        if let Ok(level) = std::env::var("RUST_LOG") {
            let level = match level.to_lowercase().as_str() {
                "error" => LevelFilter::Error,
                "warn" => LevelFilter::Warn,
                "info" => LevelFilter::Info,
                "debug" => LevelFilter::Debug,
                "trace" => LevelFilter::Trace,
                _ => LevelFilter::Off,
            };
            if log::set_logger(&LOGGER)
                .map(|()| log::set_max_level(level))
                .is_err()
            {
                // A logger has already been set, so we can't set ours.
                // This can happen if another test has already set a logger.
            }
        }
    });
}

struct TestLogger;

impl Log for TestLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            // Using eprintln so it doesn't interfere with test output captured by the test runner
            eprintln!(
                "[{}] {} - {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;
