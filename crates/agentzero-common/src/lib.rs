use tracing_subscriber::EnvFilter;

pub fn init_tracing(verbosity: u8) {
    let level = verbosity_to_level(verbosity);
    std::env::set_var("RUST_LOG", level);
    let filter = EnvFilter::new(level);

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init()
        .ok();
}

fn verbosity_to_level(verbosity: u8) -> &'static str {
    match verbosity {
        0 | 1 => "error",
        2 => "info",
        3 => "debug",
        _ => "trace",
    }
}

#[cfg(test)]
mod tests {
    use super::verbosity_to_level;

    #[test]
    fn verbosity_level_one_maps_to_error() {
        assert_eq!(verbosity_to_level(1), "error");
    }

    #[test]
    fn verbosity_level_two_maps_to_info() {
        assert_eq!(verbosity_to_level(2), "info");
    }

    #[test]
    fn verbosity_level_three_maps_to_debug() {
        assert_eq!(verbosity_to_level(3), "debug");
    }

    #[test]
    fn verbosity_level_four_or_more_maps_to_trace() {
        assert_eq!(verbosity_to_level(4), "trace");
        assert_eq!(verbosity_to_level(8), "trace");
    }
}
