use std::{backtrace::Backtrace, io, sync::mpsc::Receiver};

use crossterm::{
    event::{DisableMouseCapture, EventStream},
    execute,
    terminal::{self, EnterAlternateScreen},
};
use futures_util::{Stream, StreamExt as _};
use ratatui::{
    Terminal,
    prelude::{Backend, CrosstermBackend},
};

pub async fn run() -> anyhow::Result<()> {
    let (panic_tx, panic_rx) = std::sync::mpsc::sync_channel(1);
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = Backtrace::capture();
        let _ = panic_tx.send((info.to_string(), backtrace));
    }));

    let term = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut app = App::new(term, panic_rx)?;
    app.enter()?;

    let events = EventStream::new();

    app.run(events).await
}

pub struct App<B: Backend + io::Write> {
    term: Terminal<B>,
    panic_rx: Receiver<(String, Backtrace)>,
}

impl<B: Backend + io::Write> App<B> {
    pub fn new(term: Terminal<B>, panic_rx: Receiver<(String, Backtrace)>) -> io::Result<Self> {
        Ok(Self { term, panic_rx })
    }

    pub fn enter(&mut self) -> io::Result<()> {
        execute!(
            self.term.backend_mut(),
            EnterAlternateScreen,
            DisableMouseCapture
        )?;
        terminal::enable_raw_mode()?;
        Ok(())
    }

    pub async fn run(
        &mut self,
        events: impl Stream<Item = io::Result<crossterm::event::Event>>,
    ) -> anyhow::Result<()> {
        let events = events.filter_map(|ev| async { ev.ok() });
        let _ = events;
        Ok(())
    }
}

impl<W: Backend + io::Write> Drop for App<W> {
    fn drop(&mut self) {
        let _ = ratatui::try_restore();

        if let Ok((panic, backtrace)) = self.panic_rx.try_recv() {
            use std::io::Write as _;
            let mut stderr = io::stderr().lock();
            let _ = writeln!(stderr, "{panic}");
            let _ = writeln!(stderr, "{backtrace}");
        }
    }
}
