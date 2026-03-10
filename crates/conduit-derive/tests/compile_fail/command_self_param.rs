use conduit_derive::command;

struct MyHandler;

impl MyHandler {
    #[command]
    fn method_handler(&self) -> Vec<u8> {
        vec![]
    }
}

fn main() {}
