use slog::warn;
use slog_scope;
use std::error::Error;

pub(crate) trait ResultExtensions<T, E> {
    fn log_failing_result(self, message: &str) -> Option<T>;
}

impl<T, E> ResultExtensions<T, E> for Result<T, E> {
    fn log_failing_result(self, message: &str) -> Option<T> {
        warn!(slog_scope::logger(), "{}", message; "error" => format!("{:?}", message));
        self.ok()
    }
}
