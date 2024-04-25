use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};

pub struct Progress;

impl Progress {
    pub fn get(msg: &'static str, length: usize) -> indicatif::ProgressBar {
        let progress_bar = ProgressBar::new(length as u64)
            .with_message(format!("{msg:<50}"))
            .with_finish(ProgressFinish::AndLeave);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise:.green}] [{eta_precise:.cyan}] {msg:.magenta} ({percent:.bold}%) [{bar:30.cyan/blue}]",
                )
                .unwrap()
                .progress_chars("█░")
        );
        progress_bar
    }
}
