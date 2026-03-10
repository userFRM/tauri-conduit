use conduit_derive::command;

#[command]
fn generic_handler<T: ToString>(val: T) -> Vec<u8> {
    val.to_string().into_bytes()
}

fn main() {}
