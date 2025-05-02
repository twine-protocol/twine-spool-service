use std::fmt::Display;
use log::{Level, Log, Metadata, Record};
use serde::Serialize;
use web_sys::console::log_1;
use worker::js_sys::Date;

// TODO: release as library

#[derive(Debug, Clone, Serialize)]
struct LogEntry {
  level: String,
  target: String,
  message: String,
  timestamp: String,
}

impl LogEntry {
  pub fn new(level: impl Display, target: impl Display, message: impl Display) -> Self {
    Self {
      level: level.to_string(),
      target: target.to_string(),
      message: message.to_string(),
      timestamp: Date::new_0().to_iso_string().into(),
    }
  }

  fn log(&self) {
    if let Ok(value) = serde_wasm_bindgen::to_value(self) {
      log_1(&value)
    }
  }
}

pub struct WebLogger;

impl WebLogger {
  pub fn init_with_level(level: Level) -> Result<(), log::SetLoggerError> {
    log::set_logger(&Self)?;
    log::set_max_level(level.to_level_filter());
    Ok(())
  }
}

impl Log for WebLogger {
  fn enabled(&self, metadata: &Metadata) -> bool {
    metadata.level() <= log::max_level()
  }

  fn log(&self, record: &Record) {
    if self.enabled(record.metadata()) {
      let entry = LogEntry::new(record.level(), record.target(), record.args());
      entry.log();
    }
  }

  fn flush(&self) {
    // No-op for web console
  }
}
