mod error_ext;

fn main() {
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
