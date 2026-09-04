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
    saved::{PresetStore, SavedCatalog},
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
    let (store, catalog, catalog_error) = match PresetStore::from_env() {
        Ok(store) => match store.load() {
            Ok(catalog) => (Some(store), catalog, None),
            Err(error) => (None, SavedCatalog::default(), Some(error.to_string())),
        },
        Err(error) => (None, SavedCatalog::default(), Some(error.to_string())),
    };
    run(snapshot, &client, store, catalog, catalog_error).await
}
async fn run(
    snapshot: herdr_grid::herdr::Snapshot,
    client: &CliClient,
    store: Option<PresetStore>,
    catalog: SavedCatalog,
    catalog_error: Option<String>,
) -> anyhow::Result<()> {
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
    let mut app = App::with_catalog(snapshot, catalog);
    if let Some(error) = catalog_error {
        app.set_error(format!(
            "Custom layouts are unavailable; the existing file will not be changed: {error}"
        ));
    }
    let mut drag = None;
    let result = loop {
        app.expire_message();
        let mut geo = Default::default();
        term.draw(|f| geo = draw(f, &app))?;
        if event::poll(Duration::from_millis(250))? {
            let action = match event::read()? {
                Event::Key(k) => key(&mut app, k),
                Event::Mouse(m) => mouse(&mut app, m, &geo, &mut drag),
                _ => Action::Continue,
            };
            if app.has_catalog_change() {
                match store.as_ref() {
                    Some(store) => match store.save(&app.saved_catalog) {
                        Ok(()) => app.catalog_saved(),
                        Err(error) => app.catalog_save_failed(error),
                    },
                    None => app.catalog_save_failed(
                        "storage is disabled because the catalog could not be loaded",
                    ),
                }
            }
            match action {
                Action::Continue => {}
                Action::Cancel => break Ok(()),
                Action::Apply => {
                    if app.preview == app.snapshot.tree {
                        break Ok(());
                    }
                    let snapshot = app.snapshot.clone();
                    let target = app.preview.clone();
                    let mut render_error = None;
                    let mut report_progress = |progress| {
                        app.progress = Some(progress);
                        if let Err(error) = term.draw(|frame| {
                            let _ = draw(frame, &app);
                        }) {
                            render_error.get_or_insert_with(|| error.to_string());
                        }
                    };
                    let result = Transaction {
                        client,
                        snapshot: &snapshot,
                    }
                    .apply_with_progress(&target, &mut report_progress)
                    .await;
                    if let Some(error) = render_error {
                        break Err(anyhow::anyhow!("render apply progress: {error}"));
                    }
                    match result {
                        Ok(()) => break Ok(()),
                        Err(error) => {
                            app.progress = None;
                            app.set_error(error);
                        }
                    }
                }
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
