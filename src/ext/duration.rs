use std::time::Duration;

pub trait DurationFormat {
    fn log_str(&self) -> String;
}

impl DurationFormat for Duration {
    fn log_str(&self) -> String {
        if self.as_secs() > 0 {
            return format!("{}s", self.as_secs());
        }
        if self.as_millis() > 0 {
            return format!("{}ms", self.as_millis());
        }
        if self.as_micros() > 0 {
            return format!("{}µs", self.as_micros());
        }
        if self.as_nanos() > 0 {
            return format!("{}ns", self.as_nanos());
        }
        return format!("{:?}", self);
    }
}
