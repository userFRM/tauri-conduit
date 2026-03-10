const COMMANDS: &[&str] = &["conduit_bootstrap", "conduit_subscribe"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
