use crate::agent::{BatchAgent, MortalBatchAgent};
use crate::mjai::{Event, EventExt};
use crate::state::PlayerState;

use anyhow::{Context, Result};
use pyo3::prelude::*;
use serde::Deserialize;
use serde_json as json;

#[pyclass]
#[pyo3(text_signature = "(engine, player_id)")]
pub struct Bot {
    agent: MortalBatchAgent,
    state: PlayerState,
    log: Vec<EventExt>,
}

#[derive(Deserialize)]
struct EventWithCanAct {
    // Useful in reconnections where all previous missed logs will be replayed
    // (mjsoul)
    can_act: Option<bool>,

    #[serde(flatten)]
    event: Event,
}

#[pymethods]
impl Bot {
    #[new]
    fn new(engine: PyObject, player_id: u8) -> Result<Self> {
        let agent = MortalBatchAgent::new(engine, &[player_id])?;
        let state = PlayerState::new(player_id);
        Ok(Self {
            agent,
            state,
            log: vec![],
        })
    }

    /// Returns the reaction to `line`, if it can react, `None` otherwise.
    ///
    /// Set `can_act` to False to force the bot to only update its state without
    /// making any reaction.
    ///
    /// Both `line` and the return value are JSON strings representing one
    /// single mjai event.
    #[pyo3(name = "react")]
    #[pyo3(text_signature = "($self, line, /, *, can_act=True)")]
    #[args("*", can_act = "true")]
    fn react_py(&mut self, line: &str, can_act: bool, py: Python) -> Result<Option<String>> {
        py.allow_threads(move || self.react(line, can_act))
    }
}

impl Bot {
    fn react(&mut self, line: &str, can_act: bool) -> Result<Option<String>> {
        let data: EventWithCanAct =
            json::from_str(line).with_context(|| format!("failed to parse event {line}"))?;

        match data.event {
            Event::StartGame { .. } => {
                self.agent.start_game(0)?;
            }
            Event::EndKyoku => {
                self.log.clear();
                self.agent.end_kyoku(0)?;
            }
            Event::EndGame { .. } => {
                self.agent.end_game(0, &Default::default())?;
            }
            _ => {
                self.log.push(EventExt::no_meta(data.event.clone()));
            }
        };

        let cans = self.state.update(&data.event);
        if !can_act || matches!(data.can_act, Some(false)) || !cans.can_act() {
            return Ok(None);
        }

        self.agent
            .set_scene(0, &self.log, &self.state, None)
            .context("failed to add state")?;
        let reaction = self
            .agent
            .get_reaction(0, &self.log, &self.state, None)
            .context("failed to get reaction")?;

        let ret = json::to_string(&reaction)?;
        Ok(Some(ret))
    }
}