use slog::{crit, debug, error, info, trace, warn, Level};
use slog_scope;
use std::convert::TryFrom;
use std::num::ParseIntError;
use std::str::FromStr;

pub(crate) trait ResultExtensions<T, E> {
    fn log_failing_result_at(self, level: Level, message: &str) -> Option<T>
    where
        E: std::fmt::Debug;
    fn log_failing_result(self, message: &str) -> Option<T>
    where
        Self: Sized,
        E: std::fmt::Debug,
    {
        self.log_failing_result_at(Level::Warning, message)
    }
}

impl<T, E: std::fmt::Debug> ResultExtensions<T, E> for Result<T, E> {
    fn log_failing_result_at(self, level: Level, message: &str) -> Option<T> {
        match self {
            Ok(v) => Some(v),
            Err(e) => {
                match level {
                    Level::Warning => {
                        warn!(slog_scope::logger(), "{}", message; "error" => ?e);
                    }
                    Level::Error => {
                        error!(slog_scope::logger(), "{}", message; "error" => ?e);
                    }
                    Level::Critical => {
                        crit!(slog_scope::logger(), "{}", message; "error" => ?e);
                    }
                    Level::Info => {
                        info!(slog_scope::logger(), "{}", message; "error" => ?e);
                    }
                    Level::Debug => {
                        debug!(slog_scope::logger(), "{}", message; "error" => ?e);
                    }
                    Level::Trace => {
                        trace!(slog_scope::logger(), "{}", message; "error" => ?e);
                    }
                }
                None
            }
        }
    }
}

pub trait Numberish {
    fn parse_numberish<T: TryFrom<u64>>(&self) -> Result<T, ParseIntError>;
}

impl Numberish for str {
    fn parse_numberish<T: TryFrom<u64>>(&self) -> Result<T, ParseIntError> {
        let inu64 = if let Some(number) = self.strip_prefix("0x") {
            u64::from_str_radix(number.trim_start_matches("0"), 16)?
        } else {
            self.parse()?
        };

        match T::try_from(inu64) {
            Ok(v) => Ok(v),
            Err(_) => Err(u8::from_str("257").unwrap_err()),
        }
    }
}
