use anyhow::Context;
use clap::Parser;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
    },
};
use herdr_grid::{
    app::App,
    herdr::{CliClient, HerdrClient, Transaction},
    ui::{draw, key, mouse, Action},
};
use std::{io::stdout, time::Duration};

#[derive(Parser)]
struct Args {
    #[arg(
        long,
        help = "Print the snapshot as JSON-compatible debug output and exit"
    )]
    inspect: bool,
    #[arg(
        long,
        hide = true,
        help = "Open the plugin UI as a session-modal Herdr popup"
    )]
    open_popup: bool,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if std::env::var("HERDR_ENV").as_deref() != Ok("1") {
        anyhow::bail!("herdr-grid must run inside a Herdr-managed pane (HERDR_ENV=1)")
    }
    if args.open_popup {
        return CliClient::open_popup().await;
    }
    let client = CliClient;
    let snapshot = client.snapshot().await.context("load active Herdr tab")?;
    if args.inspect {
        println!(
            "{} panes in {}",
            snapshot.tree.pane_ids().len(),
            snapshot.tab_id
        );
        return Ok(());
    }
    run(snapshot, &client).await
}
async fn run(snapshot: herdr_grid::herdr::Snapshot, client: &CliClient) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(
        out,
        SetTitle("Layout grid"),
        EnterAlternateScreen,
        event::EnableMouseCapture
    )?;
    let _cleanup = TerminalCleanup;
    let mut term = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(out))?;
    let mut app = App::new(snapshot);
    let mut drag = None;
    let result = loop {
        let mut geo = Default::default();
        term.draw(|f| geo = draw(f, &app))?;
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(k) => match key(&mut app, k) {
                    Action::Continue => {}
                    Action::Cancel => break Ok(()),
                    Action::Apply => {
                        if app.preview == app.snapshot.tree {
                            break Ok(());
                        }
                        let snapshot = app.snapshot.clone();
                        let target = app.preview.clone();
                        let transaction = Transaction {
                            client,
                            snapshot: &snapshot,
                        };
                        let mut render_error = None;
                        let result = transaction
                            .apply_with_progress(&target, &mut |progress| {
                                app.progress = Some(progress);
                                if let Err(error) = term.draw(|frame| {
                                    let _ = draw(frame, &app);
                                }) {
                                    render_error.get_or_insert_with(|| error.to_string());
                                }
                            })
                            .await;
                        if let Some(error) = render_error {
                            break Err(anyhow::anyhow!("render apply progress: {error}"));
                        }
                        match result {
                            Ok(()) => break Ok(()),
                            Err(error) => {
                                app.progress = None;
                                app.message = Some(error.to_string());
                            }
                        }
                    }
                },
                Event::Mouse(m) => mouse(&mut app, m, &geo, &mut drag),
                _ => {}
            }
        }
    };
    result
}

struct TerminalCleanup;
impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, event::DisableMouseCapture);
    }
}
