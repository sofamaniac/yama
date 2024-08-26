use color_eyre::eyre::{eyre, Result};

use crate::ui::Ui;

pub enum Move {
    Little(MoveDirection),
    Big(MoveDirection),
    Enter,
}

pub enum MoveDirection {
    Down,
    Up,
    Left,
    Right,
}

pub enum PlayerCtrl {
    Pause,
    Play,
    Shuffle,
    Repeat,
    Auto,
    SeekForwardRel,
    SeekBackwardRel,
    SeekPercent(u32),
    VolumeUp,
    VolumeDown,
    Prev,
    Next
}

pub async fn handle(ui: &mut Ui, key: crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode as KC;
    type MD = MoveDirection;

    match key.code {
        // TODO: move key to configuration
        KC::Char('q') => ui.quit = true,

        KC::Char('j') => ui.movement(Move::Little(MD::Down)).await,
        KC::Char('k') => ui.movement(Move::Little(MD::Up)).await,
        KC::Char('h') => ui.movement(Move::Little(MD::Left)).await,
        KC::Char('l') => ui.movement(Move::Little(MD::Right)).await,

        KC::Char('J') => ui.movement(Move::Big(MD::Down)).await,
        KC::Char('K') => ui.movement(Move::Big(MD::Up)).await,
        KC::Char('H') => ui.movement(Move::Big(MD::Left)).await,
        KC::Char('L') => ui.movement(Move::Big(MD::Right)).await,
        KC::Enter => ui.movement(Move::Enter).await,

        KC::Char(' ') => ui.player_ctrl(PlayerCtrl::Pause).await,
        KC::Char('a') => ui.player_ctrl(PlayerCtrl::Auto).await,
        KC::Char('r') => ui.player_ctrl(PlayerCtrl::Repeat).await,
        KC::Char('y') => ui.player_ctrl(PlayerCtrl::Shuffle).await,

        KC::Char('d') => ui.player_ctrl(PlayerCtrl::VolumeDown).await,
        KC::Char('f') => ui.player_ctrl(PlayerCtrl::VolumeUp).await,

        KC::Char('>') => ui.player_ctrl(PlayerCtrl::Next).await,
        KC::Char('<') => ui.player_ctrl(PlayerCtrl::Prev).await,

        KC::Right => ui.player_ctrl(PlayerCtrl::SeekForwardRel).await,
        KC::Left => ui.player_ctrl(PlayerCtrl::SeekBackwardRel).await,

        KC::Char('&') => ui.player_ctrl(PlayerCtrl::SeekPercent(10)).await,
        KC::Char('é') => ui.player_ctrl(PlayerCtrl::SeekPercent(20)).await,
        KC::Char('"') => ui.player_ctrl(PlayerCtrl::SeekPercent(30)).await,
        KC::Char('\'') => ui.player_ctrl(PlayerCtrl::SeekPercent(40)).await,
        KC::Char('(') => ui.player_ctrl(PlayerCtrl::SeekPercent(50)).await,
        KC::Char('-') => ui.player_ctrl(PlayerCtrl::SeekPercent(60)).await,
        KC::Char('è') => ui.player_ctrl(PlayerCtrl::SeekPercent(70)).await,
        KC::Char('_') => ui.player_ctrl(PlayerCtrl::SeekPercent(80)).await,
        KC::Char('ç') => ui.player_ctrl(PlayerCtrl::SeekPercent(90)).await,
        KC::Char('à') => ui.player_ctrl(PlayerCtrl::SeekPercent(0)).await,
        _ => (),
    }
}
