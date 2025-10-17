//! Logging infrastructure for bitkit-core

use std::sync::Arc;
use once_cell::sync::OnceCell;

/// Global logger instance
static LOGGER: OnceCell<Arc<Logger>> = OnceCell::new();

/// Get the global logger instance
pub(crate) fn get_logger() -> Option<&'static Arc<Logger>> {
    LOGGER.get()
}

/// Set the global logger instance
///
/// This should be called once during application initialization.
/// Subsequent calls will be ignored.
pub fn set_logger(log_writer: Arc<dyn LogWriter>) {
    LOGGER.get_or_init(|| Arc::new(Logger::new_custom_writer(log_writer)));
}

/// Log level for filtering log messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, uniffi::Enum)]
pub enum LogLevel {
    /// Trace-level logs (most verbose)
    Trace,
    /// Debug-level logs
    Debug,
    /// Info-level logs
    Info,
    /// Warning-level logs
    Warn,
    /// Error-level logs (least verbose)
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "TRACE"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// A log record containing metadata and the log message
#[derive(Debug, Clone, uniffi::Record)]
pub struct LogRecord {
    /// The log level
    pub level: LogLevel,
    /// The formatted log message
    pub message: String,
    /// The module path where the log was generated
    pub module_path: String,
    /// The line number where the log was generated
    pub line: u32,
}

/// Trait for custom log writers that can be implemented by FFI consumers
///
/// This trait should be implemented by Kotlin/Swift code to receive log messages
/// from the Rust library. When Rust code logs, it will call the `log` method
/// on the registered LogWriter implementation.
#[uniffi::export(with_foreign)]
pub trait LogWriter: Send + Sync {
    /// Process a log record
    fn log(&self, record: LogRecord);
}

/// Internal logger implementation
pub(crate) struct Logger {
    writer: Arc<dyn LogWriter>,
}

impl Logger {
    /// Create a new logger with a custom writer
    pub fn new_custom_writer(log_writer: Arc<dyn LogWriter>) -> Self {
        Self { writer: log_writer }
    }

    /// Log a message
    pub fn log(&self, record: LogRecord) {
        self.writer.log(record);
    }
}

/// Macro for trace-level logging
#[macro_export]
macro_rules! log_trace {
    ($logger:expr, $($arg:tt)*) => {
        if let Some(logger) = $logger.as_ref() {
            logger.log($crate::logger::LogRecord {
                level: $crate::logger::LogLevel::Trace,
                message: format!($($arg)*),
                module_path: module_path!().to_string(),
                line: line!(),
            });
        }
    };
}

/// Macro for debug-level logging
#[macro_export]
macro_rules! log_debug {
    ($logger:expr, $($arg:tt)*) => {
        if let Some(logger) = $logger.as_ref() {
            logger.log($crate::logger::LogRecord {
                level: $crate::logger::LogLevel::Debug,
                message: format!($($arg)*),
                module_path: module_path!().to_string(),
                line: line!(),
            });
        }
    };
}

/// Macro for info-level logging
#[macro_export]
macro_rules! log_info {
    ($logger:expr, $($arg:tt)*) => {
        if let Some(logger) = $logger.as_ref() {
            logger.log($crate::logger::LogRecord {
                level: $crate::logger::LogLevel::Info,
                message: format!($($arg)*),
                module_path: module_path!().to_string(),
                line: line!(),
            });
        }
    };
}

/// Macro for warning-level logging
#[macro_export]
macro_rules! log_warn {
    ($logger:expr, $($arg:tt)*) => {
        if let Some(logger) = $logger.as_ref() {
            logger.log($crate::logger::LogRecord {
                level: $crate::logger::LogLevel::Warn,
                message: format!($($arg)*),
                module_path: module_path!().to_string(),
                line: line!(),
            });
        }
    };
}

/// Macro for error-level logging
#[macro_export]
macro_rules! log_error {
    ($logger:expr, $($arg:tt)*) => {
        if let Some(logger) = $logger.as_ref() {
            logger.log($crate::logger::LogRecord {
                level: $crate::logger::LogLevel::Error,
                message: format!($($arg)*),
                module_path: module_path!().to_string(),
                line: line!(),
            });
        }
    };
}
