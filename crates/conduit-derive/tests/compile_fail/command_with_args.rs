use conduit_derive::command;

#[command(rename = "foo")]
fn handler() -> Vec<u8> {
    vec![]
}

fn main() {}
