//! Binary entrypoint. See `cli` for the clap surface.

fn main() -> anyhow::Result<()> {
    reclamo_anl::cli::main()
}
