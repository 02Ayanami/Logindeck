//! Versioned, data-only login adapters. No script, shell, filesystem, network or
//! credential-reference operations are representable in this format.
use crate::Platform;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    schema_version: u32,
    id: String,
    revision: u32,
    platform: Platform,
    bundle_id: String,
    states: Vec<LoginState>,
}
#[derive(Clone, Debug)]
pub struct AppDefinition(Definition);
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginState {
    pub id: String,
    pub kind: StateKind,
    pub controls: BTreeMap<Control, Selector>,
    pub actions: Vec<Action>,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StateKind {
    PasswordLogin,
    Manual,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Username,
    Password,
    Submit,
    Marker,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Fill { field: CredentialField },
    RequestUser,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialField {
    Username,
    Password,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    TextField,
    SecureTextField,
    Button,
    Checkbox,
    MenuItem,
    Menu,
    StaticText,
    Group,
    WebArea,
    Window,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    role: Role,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    identifier: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    role: Role,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    identifier: Option<String>,
    #[serde(default)]
    ancestor: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    descendant: Option<Anchor>,
}
/// Read-only accessibility metadata. Never populate it with AXValue/passwords.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub role: Option<Role>,
    pub names: Vec<String>,
    pub identifier: Option<String>,
    pub parent: Option<usize>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterError {
    Invalid,
    UnsupportedVersion,
    UnknownState,
    Ambiguous,
}
pub struct Detection<'a> {
    pub state: &'a LoginState,
    pub controls: BTreeMap<Control, usize>,
}
impl AppDefinition {
    pub fn parse(json: &str) -> Result<Self, AdapterError> {
        if json.len() > 64 * 1024 {
            return Err(AdapterError::Invalid);
        }
        let definition: Definition =
            serde_json::from_str(json).map_err(|_| AdapterError::Invalid)?;
        if definition.schema_version != 1 {
            return Err(AdapterError::UnsupportedVersion);
        }
        if !identifier(&definition.id)
            || definition.revision == 0
            || !identifier(&definition.bundle_id)
            || definition.states.is_empty()
            || definition.states.len() > 8
        {
            return Err(AdapterError::Invalid);
        }
        let mut ids = BTreeSet::new();
        let mut login_states = 0;
        for state in &definition.states {
            if !identifier(&state.id)
                || !ids.insert(&state.id)
                || state.controls.is_empty()
                || state.controls.len() > 4
            {
                return Err(AdapterError::Invalid);
            }
            for selector in state.controls.values() {
                if !selector.valid() {
                    return Err(AdapterError::Invalid);
                }
                if let Some(ancestor) = &selector.ancestor {
                    if !valid_labels(&ancestor.names, &ancestor.identifier, true) {
                        return Err(AdapterError::Invalid);
                    }
                }
            }
            match state.kind {
                StateKind::PasswordLogin => {
                    login_states += 1;
                    if state.controls.len() != 3
                        || state.controls.get(&Control::Username).map(|s| s.role)
                            != Some(Role::TextField)
                        || state.controls.get(&Control::Password).map(|s| s.role)
                            != Some(Role::SecureTextField)
                        || state.controls.get(&Control::Submit).map(|s| s.role)
                            != Some(Role::Button)
                        || state.actions
                            != [
                                Action::Fill {
                                    field: CredentialField::Username,
                                },
                                Action::Fill {
                                    field: CredentialField::Password,
                                },
                            ]
                    {
                        return Err(AdapterError::Invalid);
                    }
                }
                StateKind::Manual => {
                    if state.actions != [Action::RequestUser]
                        || state.controls.keys().any(|c| *c != Control::Marker)
                    {
                        return Err(AdapterError::Invalid);
                    }
                }
            }
        }
        if login_states != 1 {
            return Err(AdapterError::Invalid);
        }
        Ok(Self(definition))
    }
    pub fn id(&self) -> &str {
        &self.0.id
    }
    pub fn revision(&self) -> u32 {
        self.0.revision
    }
    pub fn bundle_id(&self) -> &str {
        &self.0.bundle_id
    }
    pub fn supports(&self, platform: Platform, bundle_id: &str) -> bool {
        self.0.platform == platform && self.0.bundle_id == bundle_id
    }
    pub fn detect<'a>(&'a self, nodes: &[Node]) -> Result<Detection<'a>, AdapterError> {
        if nodes.len() > 512
            || nodes
                .iter()
                .enumerate()
                .any(|(i, n)| n.parent.is_some_and(|p| p >= i))
        {
            return Err(AdapterError::Invalid);
        }
        let mut detections = Vec::new();
        for state in &self.0.states {
            let candidates: Vec<_> = state
                .controls
                .iter()
                .map(|(control, selector)| {
                    (
                        *control,
                        nodes
                            .iter()
                            .enumerate()
                            .filter_map(|(i, node)| selector.matches(i, node, nodes).then_some(i))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            // An incomplete state does not shadow a complete state, but a complete
            // state with multiple matching controls is unsafe even if another is unique.
            if candidates.iter().any(|(_, matches)| matches.is_empty()) {
                continue;
            }
            if candidates.iter().any(|(_, matches)| matches.len() != 1) {
                return Err(AdapterError::Ambiguous);
            }
            detections.push(Detection {
                state,
                controls: candidates.into_iter().map(|(c, m)| (c, m[0])).collect(),
            });
        }
        if detections.len() > 1 {
            return Err(AdapterError::Ambiguous);
        }
        detections.pop().ok_or(AdapterError::UnknownState)
    }
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn valid_labels(names: &[String], id: &Option<String>, allow_empty: bool) -> bool {
    (allow_empty || !names.is_empty() || id.is_some())
        && names.len() <= 8
        && names.iter().all(|n| !n.trim().is_empty() && n.len() <= 256)
        && id
            .as_ref()
            .is_none_or(|s| !s.trim().is_empty() && s.len() <= 256)
}
fn matches(role: Role, names: &[String], id: &Option<String>, node: &Node) -> bool {
    node.role == Some(role)
        && (names.is_empty() || names.iter().any(|name| node.names.contains(name)))
        && id
            .as_ref()
            .is_none_or(|value| node.identifier.as_ref() == Some(value))
}
impl Selector {
    /// Resolve a saved selector against a fresh snapshot; never use a stale index.
    pub fn locate(&self, nodes: &[Node]) -> Result<usize, AdapterError> {
        if !self.valid()
            || nodes.len() > 512
            || nodes
                .iter()
                .enumerate()
                .any(|(i, n)| n.parent.is_some_and(|p| p >= i))
        {
            return Err(AdapterError::Invalid);
        }
        let mut matches = nodes
            .iter()
            .enumerate()
            .filter(|(i, node)| self.matches(*i, node, nodes));
        let (index, _) = matches.next().ok_or(AdapterError::UnknownState)?;
        if matches.next().is_some() {
            return Err(AdapterError::Ambiguous);
        }
        Ok(index)
    }
    /// Builder selection from one local snapshot. Never records input values.
    /// Prefer an identifier; otherwise a name and an ancestor must disambiguate.
    pub fn from_node(nodes: &[Node], index: usize) -> Result<Self, AdapterError> {
        if nodes.len() > 512
            || nodes
                .iter()
                .enumerate()
                .any(|(i, n)| n.parent.is_some_and(|p| p >= i))
        {
            return Err(AdapterError::Invalid);
        }
        let node = nodes.get(index).ok_or(AdapterError::Invalid)?;
        let role = node.role.ok_or(AdapterError::Invalid)?;
        let identifier = node
            .identifier
            .clone()
            .filter(|s| !s.trim().is_empty() && s.len() <= 256);
        let names = if identifier.is_some() {
            vec![]
        } else {
            node.names
                .iter()
                .find(|s| !s.trim().is_empty() && s.len() <= 256)
                .cloned()
                .into_iter()
                .collect()
        };
        let mut candidate = Self {
            role,
            identifier,
            names,
            ancestor: None,
            descendant: None,
        };
        if !candidate.valid() {
            return Err(AdapterError::Invalid);
        }
        let unique = |s: &Self| {
            nodes
                .iter()
                .enumerate()
                .filter(|(i, n)| s.matches(*i, n, nodes))
                .count()
                == 1
        };
        if unique(&candidate) {
            return Ok(candidate);
        }
        let mut parent = node.parent;
        for _ in 0..24 {
            let Some(index) = parent else {
                break;
            };
            let ancestor = &nodes[index];
            if let Some(role) = ancestor.role {
                let identifier = ancestor
                    .identifier
                    .clone()
                    .filter(|s| !s.trim().is_empty() && s.len() <= 256);
                let names = if identifier.is_some() {
                    vec![]
                } else {
                    ancestor
                        .names
                        .iter()
                        .find(|s| !s.trim().is_empty() && s.len() <= 256)
                        .cloned()
                        .into_iter()
                        .collect()
                };
                candidate.ancestor = Some(Anchor {
                    role,
                    names,
                    identifier,
                });
                if candidate.valid() && unique(&candidate) {
                    return Ok(candidate);
                }
            }
            parent = ancestor.parent;
        }
        candidate.ancestor = None;
        // Some apps wrap a decorative same-name button in the actionable one.
        // Record the descendant relationship to select the outer control.
        for (_, child) in nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| is_descendant(nodes, *i, index))
        {
            let Some(role) = child.role else { continue };
            candidate.descendant = Some(Anchor {
                role,
                names: child
                    .names
                    .iter()
                    .filter(|s| !s.trim().is_empty() && s.len() <= 256)
                    .take(1)
                    .cloned()
                    .collect(),
                identifier: child
                    .identifier
                    .clone()
                    .filter(|s| !s.trim().is_empty() && s.len() <= 256),
            });
            if candidate.valid() && unique(&candidate) {
                return Ok(candidate);
            }
        }
        Err(AdapterError::Ambiguous)
    }

    pub(crate) fn valid(&self) -> bool {
        valid_labels(&self.names, &self.identifier, false)
            && self
                .ancestor
                .as_ref()
                .is_none_or(|a| valid_labels(&a.names, &a.identifier, true))
            && self
                .descendant
                .as_ref()
                .is_none_or(|a| valid_labels(&a.names, &a.identifier, true))
    }
    pub fn role(&self) -> Role {
        self.role
    }
    pub(crate) fn matches(&self, index: usize, node: &Node, nodes: &[Node]) -> bool {
        if !matches(self.role, &self.names, &self.identifier, node) {
            return false;
        }
        if let Some(child) = &self.descendant {
            if !nodes.iter().enumerate().any(|(i, n)| {
                is_descendant(nodes, i, index)
                    && matches(child.role, &child.names, &child.identifier, n)
            }) {
                return false;
            }
        }
        if let Some(anchor) = &self.ancestor {
            let mut parent = node.parent;
            for _ in 0..24 {
                let Some(index) = parent else {
                    return false;
                };
                let Some(ancestor) = nodes.get(index) else {
                    return false;
                };
                if matches(anchor.role, &anchor.names, &anchor.identifier, ancestor) {
                    return true;
                }
                parent = ancestor.parent;
            }
            return false;
        }
        true
    }
}
fn is_descendant(nodes: &[Node], index: usize, ancestor: usize) -> bool {
    let mut parent = nodes[index].parent;
    for _ in 0..24 {
        let Some(i) = parent else { return false };
        if i == ancestor {
            return true;
        }
        let Some(node) = nodes.get(i) else {
            return false;
        };
        parent = node.parent;
    }
    false
}
/// Only reviewed, bundled configurations are executable in v1. Adding an adapter
/// needs a registry entry, not another recognizer/executor implementation.
pub fn builtin_for_application(
    platform: Platform,
    bundle_id: &str,
) -> Result<Option<AppDefinition>, AdapterError> {
    let mut found = None;
    const BUILTINS: &[&str] = &[include_str!("../../../adapters/qq-macos.json")];
    for source in BUILTINS {
        let definition = AppDefinition::parse(source)?;
        if definition.supports(platform, bundle_id) {
            if found.is_some() {
                return Err(AdapterError::Ambiguous);
            }
            found = Some(definition);
        }
    }
    Ok(found)
}
