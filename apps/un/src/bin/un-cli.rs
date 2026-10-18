//! `un-cli lesson --strategy unmath-count-to-three ...`: the command line with the unmath lessons
//! (`un-cli strategies` lists them).

fn main() -> anyhow::Result<()> {
    un_app::set_registry(un_mobile::registry);
    un_cli::main()
}
