const COMMANDS: &[&str] = &["bootstrap", "conduit_subscribe"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
