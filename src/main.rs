//! The `broker` binary.

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

use broker::api::fossa::ApiKey;
use clap::Parser;
use url::Url;

#[derive(Debug, Parser)]
#[command(version, about)]
struct BaseArgs {
    /// URL of FOSSA instance with which Broker should communicate.
    #[arg(short = 'e', long, default_value = "https://app.fossa.com")]
    endpoint: Url,

    /// The API key to use when communicating with FOSSA.
    #[arg(short = 'k', long = "fossa-api-key", env = "FOSSA_API_KEY")]
    api_key: ApiKey,
}

fn main() {
    let _ = BaseArgs::parse();
    let hello = hello_text();
    println!("{hello}");
}

fn hello_text() -> String {
    "Hello from Broker!".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_world_text() {
        assert_eq!(hello_text(), "Hello from Broker!".to_string());
    }
}
