use crate::{
    app_config::AppConfig,
    audio::audio_manager::{self, AudioManager},
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
/// This file has all code related to TUI.
use ratatui::{
    DefaultTerminal, Frame,
    layout::Rect,
    symbols::border,
    widgets::{Paragraph, Widget},
};
use ratatui::{prelude::*, widgets::Block};

#[derive(Debug)]
pub struct App {
    audio_manager: audio_manager::AudioManager,
    config: AppConfig,
    exit: bool,
    pub counter: i32,
}
impl App {
    pub fn new(audio_manager: AudioManager, config: AppConfig) -> Self {
        Self {
            audio_manager,
            config,
            exit: false,
            counter: 0,
        }
    }
    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> std::io::Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit = true,
            KeyCode::Left => self.counter -= 1,
            KeyCode::Right => self.counter += 1,
            KeyCode::Char('c') => self.audio_manager.join_room(10),
            KeyCode::Char('v') => self.audio_manager.exit_room(),
            KeyCode::Char('m') => self
                .audio_manager
                .set_muted(!self.audio_manager.get_muted()),

            _ => {}
        }
    }
}
impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = Line::from(" Counter App Tutorial ".bold());
        let instructions = Line::from(vec![
            " Decrement ".into(),
            "<Left>".blue().bold(),
            " Increment ".into(),
            "<Right>".blue().bold(),
            " Quit ".into(),
            "<Q> ".blue().bold(),
        ]);
        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);

        let counter_text = Text::from(vec![
            Line::from(vec!["Value: ".into(), self.counter.to_string().yellow()]),
            Line::from(
                if self.audio_manager.get_active() && !self.audio_manager.is_errored() {
                    "Now recording audio..."
                } else {
                    "Audio recording stopped: "
                },
            ),
            Line::from(if self.audio_manager.get_muted() {
                "Press M to unmute"
            } else {
                "Press M to mute self"
            }),
        ]);
        Paragraph::new(counter_text)
            .centered()
            .block(block.clone())
            .render(area, buf);
    }
}
