use conduit_derive::{Decode, Encode};

#[derive(Encode, Decode)]
struct Generic<T> {
    val: T,
}

fn main() {}
