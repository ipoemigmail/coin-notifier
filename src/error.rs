use derive_more::{Display, Error};

#[derive(Debug, Display, Error)]
pub enum ConfigError {
    #[display("failed to read config file")]
    ReadFile,
    #[display("failed to parse config: {reason}")]
    Parse { reason: String },
    #[display("invalid config: {field}")]
    Validation { field: String },
}

#[derive(Debug, Display, Error)]
pub enum ExchangeError {
    #[display("failed to connect to {exchange}")]
    Connection { exchange: String },
    #[display("request to {exchange} failed")]
    Request { exchange: String },
    #[display("failed to parse response from {exchange}")]
    ResponseParse { exchange: String },
    #[display("rate limit exceeded for {exchange}")]
    #[allow(dead_code)]
    RateLimit { exchange: String },
}

#[derive(Debug, Display, Error)]
pub enum StorageError {
    #[display("database migration failed")]
    Migration,
    #[display("failed to insert data")]
    Insert,
    #[display("failed to query data")]
    Query,
}

#[derive(Debug, Display, Error)]
pub enum IndicatorError {
    #[display("insufficient data: need {required}, got {available}")]
    InsufficientData { required: usize, available: usize },
    #[display("invalid parameter: {name}")]
    InvalidParameter { name: String },
}
