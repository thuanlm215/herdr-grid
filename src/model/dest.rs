use super::Rect;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DestId {
    Tab(String),
    Workspace(String),
    NewTab { workspace_id: String },
    NewWorkspace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RehomeDest {
    Tab { tab_id: String, label: String },
    NewTab { workspace_id: String },
    NewWorkspace,
}

impl RehomeDest {
    pub fn display_label(&self) -> String {
        match self {
            Self::Tab { label, .. } => label.clone(),
            Self::NewTab { .. } => "new tab".into(),
            Self::NewWorkspace => "new workspace".into(),
        }
    }

    pub fn matches_chip(&self, id: &DestId) -> bool {
        match (self, id) {
            (Self::Tab { tab_id, .. }, DestId::Tab(other)) => tab_id == other,
            (
                Self::NewTab { workspace_id },
                DestId::NewTab {
                    workspace_id: other,
                },
            ) => workspace_id == other,
            (Self::NewWorkspace, DestId::NewWorkspace) => true,
            (Self::Tab { tab_id, .. }, DestId::Workspace(workspace)) => tab_id
                .split_once(':')
                .is_some_and(|(ws, _)| ws == workspace),
            (Self::NewTab { workspace_id }, DestId::Workspace(workspace)) => {
                workspace_id == workspace
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rehome {
    pub pane_id: String,
    pub dest: RehomeDest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestChip {
    pub id: DestId,
    pub label: String,
    pub enabled: bool,
    pub current: bool,
    pub reason: Option<String>,
    pub badge: Option<String>,
}

impl DestChip {
    pub fn width(&self) -> u16 {
        let badge = self
            .badge
            .as_deref()
            .map(|value| value.chars().count() + 1)
            .unwrap_or(0);
        (self.label.chars().count() + badge + 2).clamp(5, 24) as u16
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DestChipZone {
    pub dest: DestId,
    pub rect: Rect,
}
