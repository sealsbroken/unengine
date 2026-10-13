//! The open desktop app: the engine with the demo lesson. Products with their own
//! methodics build a binary like this one and link their strategy crates.

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    un_desktop::run(un_app::registry())
}
