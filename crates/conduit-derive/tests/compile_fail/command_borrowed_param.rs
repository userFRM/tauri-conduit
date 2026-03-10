use conduit_derive::command;
#[command]
fn greet(name: &str) -> String { name.to_string() }
fn main() {}
