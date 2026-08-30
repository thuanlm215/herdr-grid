use super::{tree_from_layout, LayoutEnvelope, PaneListEnvelope, ProcessEnvelope, Snapshot};
use crate::model::{Direction, LayoutNode, SplitPath};
use async_trait::async_trait;
use std::time::Duration;
use std::{collections::HashMap, process::Stdio};
use tokio::process::Command;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

#[async_trait]
pub trait HerdrClient: Send + Sync {
    async fn snapshot(&self) -> anyhow::Result<Snapshot>;
    async fn layout_for(&self, _pane: &str) -> anyhow::Result<LayoutNode> {
        Ok(self.snapshot().await?.tree)
    }
    async fn pane_locations(&self, _workspace: &str) -> anyhow::Result<HashMap<String, String>> {
        Ok(self
            .snapshot()
            .await?
            .metadata
            .into_iter()
            .map(|(id, meta)| (id, meta.tab_id))
            .collect())
    }
    async fn swap(&self, source: &str, target: &str) -> anyhow::Result<()>;
    async fn set_ratio(&self, tab: &str, path: &SplitPath, ratio: f64) -> anyhow::Result<()>;
    async fn park_pane(&self, _pane: &str, _workspace: &str) -> anyhow::Result<MoveOutcome> {
        anyhow::bail!("structural pane parking is not implemented by this client")
    }
    async fn move_pane(
        &self,
        _pane: &str,
        _tab: &str,
        _target: &str,
        _direction: Direction,
        _ratio: f64,
    ) -> anyhow::Result<MoveOutcome> {
        anyhow::bail!("structural pane movement is not implemented by this client")
    }
}

#[derive(Clone, Debug)]
pub struct MoveOutcome {
    pub pane_id: String,
    pub tab_id: String,
    pub created_tab_id: Option<String>,
    pub target_tree: LayoutNode,
}

#[derive(Clone, Debug, Default)]
pub struct CliClient;
impl CliClient {
    pub async fn open_popup() -> anyhow::Result<()> {
        let plugin_id = std::env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-grid".into());
        Self::socket_request(
            "plugin.pane.open",
            serde_json::json!({
                "plugin_id": plugin_id,
                "entrypoint": "grid",
                "placement": "popup",
                "width": "85%",
                "height": "85%",
                "focus": true
            }),
        )
        .await
        .map(|_| ())
    }

    async fn run(args: &[&str]) -> anyhow::Result<String> {
        let binary = std::env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".into());
        let out = Command::new(binary)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await?;
        if !out.status.success() {
            anyhow::bail!(
                "herdr {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            )
        }
        Ok(String::from_utf8(out.stdout)?)
    }

    fn origin_pane() -> anyhow::Result<String> {
        if let Ok(context) = std::env::var("HERDR_PLUGIN_CONTEXT_JSON") {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&context) {
                if let Some(id) = value
                    .get("focused_pane_id")
                    .and_then(|v| v.as_str())
                    .filter(|id| !id.is_empty())
                {
                    return Ok(id.into());
                }
            }
        }
        if let Ok(id) = std::env::var("HERDR_ACTIVE_PANE_ID") {
            if !id.is_empty() {
                return Ok(id);
            }
        }
        if let Ok(id) = std::env::var("HERDR_PANE_ID") {
            if !id.is_empty() {
                return Ok(id);
            }
        }
        anyhow::bail!("cannot resolve originating Herdr pane")
    }

    async fn socket_request(
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let path = std::env::var("HERDR_SOCKET_PATH")?;
        let mut stream = tokio::time::timeout(Duration::from_secs(5), UnixStream::connect(path))
            .await
            .map_err(|_| anyhow::anyhow!("Herdr socket connect timed out"))??;
        let request = serde_json::json!({
            "id": "herdr-grid:apply",
            "method": method,
            "params": params
        });
        tokio::time::timeout(
            Duration::from_secs(5),
            stream.write_all(format!("{}\n", serde_json::to_string(&request)?).as_bytes()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Herdr socket write timed out"))??;
        let mut response = String::new();
        tokio::time::timeout(
            Duration::from_secs(5),
            BufReader::new(stream).read_line(&mut response),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Herdr socket response timed out"))??;
        let response: serde_json::Value = serde_json::from_str(&response)?;
        if let Some(error) = response.get("error") {
            anyhow::bail!("Herdr socket error: {error}")
        }
        if response.get("result").is_none() {
            anyhow::bail!("invalid Herdr socket response")
        }
        Ok(response["result"].clone())
    }

    fn move_outcome(result: serde_json::Value) -> anyhow::Result<MoveOutcome> {
        let moved = result
            .get("move_result")
            .ok_or_else(|| anyhow::anyhow!("pane.move response lacks move_result"))?;
        if moved.get("changed").and_then(|v| v.as_bool()) != Some(true) {
            anyhow::bail!(
                "pane.move reported no change: {}",
                moved.get("reason").unwrap_or(&serde_json::Value::Null)
            )
        }
        let pane_id = moved
            .pointer("/pane/pane_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("pane.move response lacks pane id"))?
            .to_owned();
        let tab_id = moved
            .pointer("/pane/tab_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("pane.move response lacks tab id"))?
            .to_owned();
        let created_tab_id = moved
            .pointer("/created_tab/tab_id")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let layout: super::WireLayout = serde_json::from_value(
            moved
                .get("target_layout")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("pane.move response lacks target layout"))?,
        )?;
        let target_tree = tree_from_layout(&layout)?;
        Ok(MoveOutcome {
            pane_id,
            tab_id,
            created_tab_id,
            target_tree,
        })
    }
    async fn read_layout(pane: &str) -> anyhow::Result<super::WireLayout> {
        let raw = Self::run(&["pane", "layout", "--pane", pane]).await?;
        Ok(serde_json::from_str::<LayoutEnvelope>(&raw)?.result.layout)
    }
}
#[async_trait]
impl HerdrClient for CliClient {
    async fn snapshot(&self) -> anyhow::Result<Snapshot> {
        let origin = Self::origin_pane()?;
        let layout = Self::read_layout(&origin).await?;
        if layout.zoomed {
            anyhow::bail!("active tab is zoomed; unzoom before editing")
        }
        let workspace = layout.workspace_id.clone();
        let list = Self::run(&["pane", "list", "--workspace", &workspace]).await?;
        let all: PaneListEnvelope = serde_json::from_str(&list)?;
        let tab = layout.tab_id.clone();
        let mut metadata: HashMap<_, _> = all
            .result
            .panes
            .into_iter()
            .filter(|p| {
                p.pane_id.starts_with(&format!("{}:p", workspace))
                    && layout.panes.iter().any(|x| x.pane_id == p.pane_id)
            })
            .map(|p| (p.pane_id.clone(), p))
            .collect();
        for (pane_id, meta) in &mut metadata {
            if let Ok(raw) = Self::run(&["pane", "process-info", "--pane", pane_id]).await {
                if let Ok(process) = serde_json::from_str::<ProcessEnvelope>(&raw) {
                    meta.process_name = process
                        .result
                        .process_info
                        .foreground_processes
                        .first()
                        .map(|p| p.name.clone());
                }
            }
        }
        let revisions = metadata
            .iter()
            .map(|(id, m)| (id.clone(), m.revision))
            .collect();
        let tree = tree_from_layout(&layout)?;
        Ok(Snapshot {
            workspace_id: workspace,
            tab_id: tab,
            focused_pane_id: layout.focused_pane_id,
            tree,
            metadata,
            revisions,
        })
    }
    async fn layout_for(&self, pane: &str) -> anyhow::Result<LayoutNode> {
        tree_from_layout(&Self::read_layout(pane).await?)
    }
    async fn pane_locations(&self, workspace: &str) -> anyhow::Result<HashMap<String, String>> {
        let raw = Self::run(&["pane", "list", "--workspace", workspace]).await?;
        let all: PaneListEnvelope = serde_json::from_str(&raw)?;
        Ok(all
            .result
            .panes
            .into_iter()
            .map(|p| (p.pane_id, p.tab_id))
            .collect())
    }
    async fn swap(&self, source: &str, target: &str) -> anyhow::Result<()> {
        Self::run(&[
            "pane",
            "swap",
            "--source-pane",
            source,
            "--target-pane",
            target,
        ])
        .await
        .map(|_| ())
    }
    async fn set_ratio(&self, tab: &str, path: &SplitPath, ratio: f64) -> anyhow::Result<()> {
        Self::socket_request(
            "layout.set_split_ratio",
            serde_json::json!({"tab_id": tab, "path": path, "ratio": ratio}),
        )
        .await
        .map(|_| ())
    }
    async fn park_pane(&self, pane: &str, workspace: &str) -> anyhow::Result<MoveOutcome> {
        let result = Self::socket_request("pane.move", serde_json::json!({"pane_id":pane,"destination":{"type":"new_tab","workspace_id":workspace,"label":"herdr-grid recovery"},"focus":false})).await?;
        Self::move_outcome(result)
    }
    async fn move_pane(
        &self,
        pane: &str,
        tab: &str,
        target: &str,
        direction: Direction,
        ratio: f64,
    ) -> anyhow::Result<MoveOutcome> {
        let split = match direction {
            Direction::Horizontal => "right",
            Direction::Vertical => "down",
        };
        let result = Self::socket_request("pane.move", serde_json::json!({"pane_id":pane,"destination":{"type":"tab","tab_id":tab,"target_pane_id":target,"split":split,"ratio":ratio},"focus":false})).await?;
        Self::move_outcome(result)
    }
}
