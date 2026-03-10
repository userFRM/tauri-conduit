use conduit_derive::command;

#[command]
fn ref_handler(ref name: String) -> Vec<u8> {
    name.as_bytes().to_vec()
}

fn main() {}
