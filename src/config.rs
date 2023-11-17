use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub secrets_location: String,
    pub yt_dlp_output_template: String,
}

impl std::default::Default for Config {
    fn default() -> Self {
        Self {
            secrets_location: String::new(),
            yt_dlp_output_template: "%(title)s.%(ext)s".to_owned(),
        }
    }
}

pub fn get() -> Config {
    confy::load("yama", None).unwrap()
}
