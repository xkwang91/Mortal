use super::action::ActionCandidate;
use super::item::{ChiPon, KawaItem};
use crate::hand::tiles_to_string;
use crate::tile::Tile;
use std::iter;

use anyhow::Result;
use pyo3::prelude::*;
use serde_json as json;
use tinyvec::ArrayVec;

/// The struct is defined here because Default doesn't have impls for big arrays
/// yet.
#[derive(Debug, Clone, derivative::Derivative)]
#[derivative(Default)]
pub(super) struct BigArrayFields {
    // Does not include aka.
    #[derivative(Default(value = "[0; 34]"))]
    pub(super) tehai: [u8; 34],

    // Does not consider yakunashi, but does consider other kinds of
    // furiten.
    #[derivative(Default(value = "[false; 34]"))]
    pub(super) waits: [bool; 34],

    #[derivative(Default(value = "[0; 34]"))]
    pub(super) dora_factor: [u8; 34],

    // For calculating `waits` and `doras_seen`.
    #[derivative(Default(value = "[0; 34]"))]
    pub(super) tiles_seen: [u8; 34],

    #[derivative(Default(value = "[false; 34]"))]
    pub(super) keep_shanten_discards: [bool; 34],

    #[derivative(Default(value = "[false; 34]"))]
    pub(super) next_shanten_discards: [bool; 34],

    #[derivative(Default(value = "[false; 34]"))]
    pub(super) forbidden_tiles: [bool; 34],

    // Used for furiten check.
    #[derivative(Default(value = "[false; 34]"))]
    pub(super) discarded_tiles: [bool; 34],
}

impl BigArrayFields {
    pub(super) fn clear(&mut self) {
        self.tehai.fill(0);
        self.waits.fill(false);
        self.dora_factor.fill(0);
        self.tiles_seen.fill(0);
        self.keep_shanten_discards.fill(false);
        self.next_shanten_discards.fill(false);
        self.forbidden_tiles.fill(false);
        self.discarded_tiles.fill(false);
    }
}

/// `PlayerState` is the core of the lib, which holds all the observable game
/// state information from a specific seat's perspective with the ability to
/// identify the legal actions the specified player can make upon an incoming
/// mjai event, along with some helper functions to build an actual agent.
#[pyclass]
#[pyo3(text_signature = "(player_id)")]
#[derive(Debug, Clone, Default)]
pub struct PlayerState {
    #[pyo3(get)]
    pub(super) player_id: u8,

    pub(super) bakaze: Tile,
    pub(super) jikaze: Tile,
    // Counts from 1, same as mjai.
    pub(super) kyoku: u8,
    pub(super) honba: u8,
    pub(super) kyotaku: u8,
    // Rotated, `scores[0]` is the score of the player.
    pub(super) scores: [i32; 4],
    pub(super) rank: u8,
    // Relative to `player_id`.
    pub(super) oya: u8,
    // Including 西入 sudden deatch.
    pub(super) is_all_last: bool,
    pub(super) dora_indicators: ArrayVec<[Tile; 5]>,

    // 24 is the theoretical max size of kawa.
    //
    // Reference: https://detail.chiebukuro.yahoo.co.jp/qa/question_detail/q1020002370
    pub(super) kawa: [ArrayVec<[Option<KawaItem>; 24]>; 4],

    // Using 34-D arrays here may be more efficient, but I don't want to mess up
    // with aka doras.
    pub(super) kawa_overview: [ArrayVec<[Tile; 24]>; 4],
    pub(super) fuuro_overview: [ArrayVec<[ArrayVec<[Tile; 4]>; 4]>; 4],
    // In this field all `Tile` are deaka'd.
    pub(super) ankan_overview: [ArrayVec<[Tile; 4]>; 4],

    pub(super) riichi_declared: [bool; 4],
    pub(super) riichi_accepted: [bool; 4],

    pub(super) tiles_left: u8,
    pub(super) intermediate_kan: ArrayVec<[Tile; 4]>,
    pub(super) intermediate_chi_pon: Option<ChiPon>,

    pub(super) shanten: i8,

    pub(super) last_self_tsumo: Option<Tile>,
    pub(super) last_kawa_tile: Option<Tile>,
    pub(super) last_cans: ActionCandidate,

    pub(super) ankan_candidates: ArrayVec<[u8; 3]>,
    pub(super) kakan_candidates: ArrayVec<[u8; 3]>,
    pub(super) chankan_chance: bool,

    pub(super) can_w_riichi: bool,
    pub(super) is_w_riichi: bool,
    pub(super) at_rinshan: bool,
    pub(super) at_ippatsu: bool,
    pub(super) at_furiten: bool,
    pub(super) to_mark_same_cycle_furiten: bool,

    // Used for 4-kan check.
    pub(super) kans_on_board: u8,

    pub(super) is_menzen: bool,
    // For agari calc, all deaka'd.
    pub(super) chis: ArrayVec<[u8; 4]>,
    pub(super) pons: ArrayVec<[u8; 4]>,
    pub(super) minkans: ArrayVec<[u8; 4]>,
    pub(super) ankans: ArrayVec<[u8; 4]>,

    // Including aka, originally for agari calc usage but also encoded as a
    // feature to the obs.
    pub(super) doras_owned: [u8; 4],
    pub(super) doras_seen: u8,

    pub(super) akas_in_hand: [bool; 3],

    // For shanten calc.
    pub(super) tehai_len_div3: u8,

    // Used in can_riichi.
    pub(super) has_next_shanten_discard: bool,

    pub(super) arrs: BigArrayFields,
}

#[pymethods]
impl PlayerState {
    #[new]
    pub fn new(player_id: u8) -> Self {
        Self {
            player_id,
            ..Default::default()
        }
    }

    /// Returns an `ActionCandidate`.
    #[pyo3(name = "update")]
    #[pyo3(text_signature = "($self, mjai_json, /)")]
    pub(super) fn update_json(&mut self, mjai_json: &str) -> Result<ActionCandidate> {
        let event = json::from_str(mjai_json)?;
        Ok(self.update(&event))
    }

    /// Raises an exception if the action is not valid.
    #[pyo3(name = "validate_action")]
    #[pyo3(text_signature = "($self, mjai_json, /)")]
    pub(super) fn validate_action_json(&self, mjai_json: &str) -> Result<()> {
        let action = json::from_str(mjai_json)?;
        self.validate_action(&action)
    }

    /// For debug only.
    ///
    /// Return a human readable description of the current state.
    #[pyo3(text_signature = "($self, /)")]
    pub fn brief_info(&self) -> String {
        let waits = self
            .arrs
            .waits
            .iter()
            .enumerate()
            .filter(|(_, &b)| b)
            .map(|(i, _)| Tile(i as u8))
            .collect::<Vec<_>>();

        let zipped_kawa = self.kawa[0]
            .iter()
            .chain(iter::repeat(&None))
            .zip(self.kawa[1].iter().chain(iter::repeat(&None)))
            .zip(self.kawa[2].iter().chain(iter::repeat(&None)))
            .zip(self.kawa[3].iter().chain(iter::repeat(&None)))
            .take_while(|row| !matches!(row, &(((None, None), None), None)))
            .enumerate()
            .map(|(i, (((a, b), c), d))| {
                format!(
                    "{i:2}. {}\t{}\t{}\t{}",
                    a.as_ref()
                        .map(|item| item.to_string())
                        .unwrap_or_else(|| "-".to_owned()),
                    b.as_ref()
                        .map(|item| item.to_string())
                        .unwrap_or_else(|| "-".to_owned()),
                    c.as_ref()
                        .map(|item| item.to_string())
                        .unwrap_or_else(|| "-".to_owned()),
                    d.as_ref()
                        .map(|item| item.to_string())
                        .unwrap_or_else(|| "-".to_owned()),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"player (abs): {}
oya (rel): {}
kyoku: {}{}-{}
jikaze: {}
score (rel): {:?}
tehai: {}
fuuro: {:?}
ankan: {:?}
tehai len: {}
shanten: {}
furiten: {}
waits: {waits:?}
dora indicators: {:?}
doras owned: {:?}
doras seen: {}
action candidates: {:#?}
last self tsumo: {:?}
last kawa tile: {:?}
tiles left: {}
kawa:
{zipped_kawa}"#,
            self.player_id,
            self.oya,
            self.bakaze,
            self.kyoku + 1,
            self.honba,
            self.jikaze,
            self.scores,
            tiles_to_string(&self.arrs.tehai, self.akas_in_hand),
            self.fuuro_overview[0],
            self.ankan_overview[0],
            self.tehai_len_div3,
            self.shanten,
            self.at_furiten,
            self.dora_indicators,
            self.doras_owned,
            self.doras_seen,
            self.last_cans,
            self.last_self_tsumo,
            self.last_kawa_tile,
            self.tiles_left,
        )
    }
}