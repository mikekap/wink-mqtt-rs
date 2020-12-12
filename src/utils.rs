use slog::{warn, error, info, debug, trace, crit, Level};
use slog_scope;

pub(crate) trait ResultExtensions<T, E> {
    fn log_failing_result_at(self, level: Level, message: &str) -> Option<T> where E: std::fmt::Debug;
    fn log_failing_result(self, message: &str) -> Option<T> where Self: Sized, E: std::fmt::Debug {
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
                    },
                    Level::Error => {
                        error!(slog_scope::logger(), "{}", message; "error" => ?e);
                    },
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
