use std::collections::HashMap;
use std::path::PathBuf;

use crossterm::event::KeyCode;
use directories::{ProjectDirs, UserDirs};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use crate::{
    client::interface::{PlayerAction, SeekMode, Volume},
    orchestrator::{Action, MenuCtrl},
};

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    keymap: HashMap<KeyCode, Action>,
    pub yt_secret_location: String,
    pub spotify_secret_location: String,
    pub folders: Vec<PathBuf>,
    pub focused_fg: Color,
    pub focused_bg: Color,
    pub focused_highlight_fg: Color,
    pub focused_highlight_bg: Color,
    pub unfocused_fg: Color,
    pub unfocused_bg: Color,
    pub unfocused_highlight_fg: Color,
    pub unfocused_highlight_bg: Color,
    pub border_focus: Color,
    pub border_unfocus: Color,
}

impl Config {
    pub fn get_action(&self, c: &KeyCode) -> Option<Action> {
        self.keymap.get(c).cloned()
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut keymap: HashMap<KeyCode, Action> = HashMap::new();
        keymap.insert(KeyCode::Char('q'), Action::Quit);
        keymap.insert(KeyCode::Esc, Action::CloseAlert);
        keymap.insert(KeyCode::Char('j'), MenuCtrl::Next.into());
        keymap.insert(KeyCode::Char('k'), MenuCtrl::Prev.into());
        keymap.insert(KeyCode::Char('l'), MenuCtrl::NextMenu.into());
        keymap.insert(KeyCode::Char('h'), MenuCtrl::PrevMenu.into());
        keymap.insert(KeyCode::Char(' '), PlayerAction::PlayPauseToggle.into());
        keymap.insert(KeyCode::Char('a'), Action::ToggleAuto);
        keymap.insert(
            KeyCode::Left,
            PlayerAction::Seek {
                dt: -5,
                mode: SeekMode::Relative,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Right,
            PlayerAction::Seek {
                dt: 5,
                mode: SeekMode::Relative,
            }
            .into(),
        );
        keymap.insert(KeyCode::Char('<'), PlayerAction::Prev.into());
        keymap.insert(KeyCode::Char('>'), PlayerAction::Next.into());
        keymap.insert(
            KeyCode::Char('d'),
            PlayerAction::SetVolume(Volume::Relative(-5)).into(),
        );
        keymap.insert(
            KeyCode::Char('f'),
            PlayerAction::SetVolume(Volume::Relative(5)).into(),
        );
        keymap.insert(KeyCode::Char('g'), Action::GoToCurrent);
        keymap.insert(KeyCode::Char('r'), PlayerAction::CycleRepeat.into());
        keymap.insert(KeyCode::Char('y'), PlayerAction::ShuffleToggle.into());
        keymap.insert(
            KeyCode::Char('&'),
            PlayerAction::Seek {
                dt: 10,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Char('é'),
            PlayerAction::Seek {
                dt: 20,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Char('"'),
            PlayerAction::Seek {
                dt: 30,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Char('\''),
            PlayerAction::Seek {
                dt: 40,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Char('('),
            PlayerAction::Seek {
                dt: 50,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Char('-'),
            PlayerAction::Seek {
                dt: 60,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Char('è'),
            PlayerAction::Seek {
                dt: 70,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Char('_'),
            PlayerAction::Seek {
                dt: 80,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Char('ç'),
            PlayerAction::Seek {
                dt: 90,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(
            KeyCode::Char('à'),
            PlayerAction::Seek {
                dt: 0,
                mode: SeekMode::AbsolutePercent,
            }
            .into(),
        );
        keymap.insert(KeyCode::Char(':'), Action::CommandPrompt);
        let dirs = get_dirs();
        let mut yt_secrets_loc: PathBuf = PathBuf::from(dirs.config_dir());
        yt_secrets_loc.push("yt_secrets.json");
        let mut spotify_secrets_loc: PathBuf = PathBuf::from(dirs.config_dir());
        spotify_secrets_loc.push("spotify_secrets.json");
        let user_dirs = UserDirs::new().unwrap();
        let audio_dir = user_dirs.audio_dir().unwrap();
        Self {
            keymap,
            yt_secret_location: format!("{}", yt_secrets_loc.display()),
            spotify_secret_location: format!("{}", spotify_secrets_loc.display()),
            folders: vec![audio_dir.into()],
            focused_fg: Color::Rgb(202, 211, 245),
            focused_bg: Color::Reset,
            focused_highlight_fg: Color::Rgb(202, 211, 245),
            focused_highlight_bg: Color::Rgb(91, 96, 120),
            unfocused_fg: Color::Rgb(110, 115, 141),
            unfocused_bg: Color::Reset,
            unfocused_highlight_fg: Color::Reset,
            unfocused_highlight_bg: Color::Rgb(110, 115, 141),
            border_focus: Color::Rgb(183, 189, 248),
            border_unfocus: Color::Rgb(110, 115, 141),
        }
    }
}

pub fn get_config() -> Config {
    confy::load("yamav3", None).unwrap_or_default()
}

pub fn get_dirs() -> ProjectDirs {
    // TODO do something better or not
    ProjectDirs::from("com", "sofamaniac", "yamav3").unwrap()
}
