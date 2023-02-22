/// Tests are run independently by cargo nextest, so this macro configures settings used in most tests.
macro_rules! set_vars {
    () => {
        // During error stack snapshot testing, colors really mess with readability.
        // While colors are an important part of the overall error message story,
        // they're less important than structure; the thought is that by making structure easier to test
        // we can avoid most failures. Colors, by comparison, are harder to accidentally change.
        error_stack::Report::set_color_mode(error_stack::fmt::ColorMode::None);
        colored::control::set_override(false);
    };
}

pub(crate) use set_vars;
