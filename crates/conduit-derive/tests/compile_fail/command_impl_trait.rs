use conduit_derive::command;

#[command]
fn impl_trait_handler(val: impl ToString) -> Vec<u8> {
    val.to_string().into_bytes()
}

fn main() {}
