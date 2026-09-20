//! Local authoring session; only explicitly selected metadata is exported.
use super::*;

pub struct Builder {
    definition: Definition,
    captures: BTreeMap<String, Vec<Node>>,
}
impl Builder {
    pub fn new(id: String, platform: Platform, application_id: String) -> Result<Self, Error> {
        if !name(&id) || !name(&application_id) {
            return Err(Error::InvalidDefinition);
        }
        Ok(Self {
            definition: Definition {
                schema_version: 3,
                id,
                platform,
                application_id,
                states: BTreeMap::new(),
                flows: BTreeMap::new(),
            },
            captures: BTreeMap::new(),
        })
    }
    pub fn from_definition(definition: Definition) -> Result<Self, Error> {
        definition.validate()?;
        Ok(Self {
            definition,
            captures: BTreeMap::new(),
        })
    }
    /// Restore a local authoring draft that may have temporarily broken graph
    /// references. The representable operations and all size limits remain bounded;
    /// `export` still performs the complete executable-definition validation.
    pub fn from_draft(source: &str) -> Result<Self, Error> {
        if source.len() > 64 * 1024 {
            return Err(Error::InvalidDefinition);
        }
        let mut value: serde_json::Value =
            serde_json::from_str(source).map_err(|_| Error::InvalidDefinition)?;
        migrate_v2(&mut value)?;
        let definition: Definition =
            serde_json::from_value(value).map_err(|_| Error::InvalidDefinition)?;
        validate_draft(&definition)?;
        Ok(Self {
            definition,
            captures: BTreeMap::new(),
        })
    }
    pub fn states(&self) -> &BTreeMap<String, State> {
        &self.definition.states
    }
    pub fn platform(&self) -> Platform {
        self.definition.platform
    }
    pub fn application_id(&self) -> &str {
        &self.definition.application_id
    }
    pub fn flows(&self) -> &BTreeMap<String, Flow> {
        &self.definition.flows
    }
    pub fn draft_json(&self) -> Result<String, Error> {
        validate_draft(&self.definition)?;
        let json =
            serde_json::to_string_pretty(&self.definition).map_err(|_| Error::InvalidDefinition)?;
        if json.len() > 64 * 1024 {
            return Err(Error::InvalidDefinition);
        }
        Ok(json)
    }
    /// Check a fresh observation against all draft states, without executing actions.
    pub fn verify_state(&self, expected: &str, nodes: &[Node]) -> Result<(), Error> {
        if self.definition.detect(nodes)?.state != expected {
            return Err(Error::UnknownState);
        }
        Ok(())
    }
    pub fn record_state(
        &mut self,
        id: String,
        kind: StateKind,
        nodes: Vec<Node>,
        selected: &[(String, usize)],
    ) -> Result<(), Error> {
        if !name(&id)
            || selected.is_empty()
            || selected.len() > 16
            || (!self.definition.states.contains_key(&id) && self.definition.states.len() >= 32)
        {
            return Err(Error::InvalidDefinition);
        }
        let mut controls = BTreeMap::new();
        for (label, index) in selected {
            if !name(label) || controls.contains_key(label) {
                return Err(Error::InvalidDefinition);
            }
            let selector = Selector::from_node(&nodes, *index).map_err(|error| match error {
                crate::adapter::AdapterError::Ambiguous => Error::Ambiguous,
                _ => Error::InvalidTree,
            })?;
            controls.insert(label.clone(), selector);
        }
        self.definition.states.insert(
            id.clone(),
            State {
                kind,
                controls,
                absent: vec![],
            },
        );
        self.captures.insert(id, nodes);
        Ok(())
    }
    /// Exclude a recorded overlay control from a background state.
    pub fn exclude_control(
        &mut self,
        state: &str,
        overlay: &str,
        control: &str,
    ) -> Result<(), Error> {
        let selector = self
            .definition
            .states
            .get(overlay)
            .and_then(|s| s.controls.get(control))
            .cloned()
            .ok_or(Error::InvalidDefinition)?;
        let target = self
            .definition
            .states
            .get_mut(state)
            .ok_or(Error::InvalidDefinition)?;
        if target.absent.len() >= 16 {
            return Err(Error::InvalidDefinition);
        }
        target.absent.push(selector);
        Ok(())
    }
    pub fn set_flow(&mut self, id: String, flow: Flow) -> Result<(), Error> {
        if !name(&id)
            || flow.steps.len() > 64
            || (!self.definition.flows.contains_key(&id) && self.definition.flows.len() >= 8)
        {
            return Err(Error::InvalidDefinition);
        }
        self.definition.flows.insert(id, flow);
        Ok(())
    }
    pub fn remove_state(&mut self, id: &str) {
        self.definition.states.remove(id);
        self.captures.remove(id);
    }
    pub fn remove_flow(&mut self, id: &str) {
        self.definition.flows.remove(id);
    }
    /// Validate the graph and all recorded examples together. A selected feature
    /// shared by two recorded pages cannot silently become a state detector.
    pub fn export(&self) -> Result<String, Error> {
        self.definition.validate()?;
        for (id, nodes) in &self.captures {
            if self.definition.detect(nodes)?.state != *id {
                return Err(Error::Ambiguous);
            }
        }
        self.definition.to_json()
    }
}

fn validate_draft(definition: &Definition) -> Result<(), Error> {
    let invalid = || Error::InvalidDefinition;
    if definition.schema_version != 3
        || !name(&definition.id)
        || !name(&definition.application_id)
        || definition.states.len() > 32
        || definition.flows.len() > 8
    {
        return Err(invalid());
    }
    for (id, state) in &definition.states {
        if !name(id)
            || state.controls.is_empty()
            || state.controls.len() > 16
            || state.absent.len() > 16
            || state.absent.iter().any(|selector| !selector.valid())
            || state
                .controls
                .iter()
                .any(|(control, selector)| !name(control) || !selector.valid())
        {
            return Err(invalid());
        }
    }
    for (id, flow) in &definition.flows {
        if !name(id) || flow.steps.len() > 64 {
            return Err(invalid());
        }
        for step in &flow.steps {
            let valid = match step {
                Step::Click {
                    state,
                    target,
                    next_states,
                    ..
                }
                | Step::Submit {
                    state,
                    target,
                    next_states,
                } => {
                    name(state)
                        && name(target)
                        && !next_states.is_empty()
                        && next_states.len() <= 8
                        && next_states.iter().all(|next| name(next))
                }
                Step::Fill { state, target, .. }
                | Step::Clear { state, target, .. }
                | Step::Check { state, target } => name(state) && name(target),
                Step::WaitState { state } => name(state),
                Step::Challenge {
                    state,
                    resume_state,
                } => name(state) && name(resume_state),
            };
            if !valid {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
