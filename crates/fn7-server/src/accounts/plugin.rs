//! [`AccountsPlugin`]: the `GamePlugin` wrapper that brings accounts into a match (docs/ACCOUNTS.md "In the match").
//!
//! It sits directly around the game plugin (inside the media wrapper), so what it returns is what the core records
//! and snapshots. It touches two hooks and forwards every other one untouched:
//!
//! - `on_match_start`: after the game builds the world, every human seat whose name an account holds, and whose
//!   engine player is bound to that account ([`super::Accounts::seat_auth`]), gets the account's persona
//!   (`fn7_plugin::persona::apply_to_state`): `seats[].account`, the two custom demigod names, the personality, style
//!   and creed. The core's `on_match_start` snapshot is the state with personas in it.
//! - `on_seat_takeover`: the same for a seat taken mid-match (the house flow), after the game's own takeover. A
//!   takeover under an account's name by a player who is **not** bound to it is refused: the game's takeover is not
//!   run, so the seat has no demigod, and a `seat_refused` event says so (the name guard removes the player).
//!
//! **Replay.** Each applied persona is the `seat_persona` trace event (the exact `SeatPersona`), each refusal the
//! `seat_refused` event: `crate::replay` feeds them back, so these matches replay byte for byte without the store.
//! A match with no account-held seat produces neither event and is exactly what it was before this wrapper existed.

use std::sync::Arc;

use async_trait::async_trait;
use fn7_plugin::persona;
use serde_json::{json, Value};
use theatre_plugin_api::*;

use super::{Accounts, SeatAuth};

pub struct AccountsPlugin<P> {
    inner: P,
    accounts: Arc<Accounts>,
}

impl<P: GamePlugin> AccountsPlugin<P> {
    pub fn new(inner: P, accounts: Arc<Accounts>) -> Self {
        Self { inner, accounts }
    }

    fn run() -> Option<String> {
        theatre_trace::current().map(|c| c.run)
    }
}

#[async_trait]
impl<P: GamePlugin> GamePlugin for AccountsPlugin<P> {
    fn info(&self) -> PluginInfo {
        self.inner.info()
    }
    fn validate_settings(&self, settings: &Value) -> Result<Value, String> {
        self.inner.validate_settings(settings)
    }
    fn queries(&self) -> Vec<QuerySpec> {
        self.inner.queries()
    }
    fn query(&self, state: &Value, seat: Option<Seat>, name: &str, args: &Value) -> Result<Value, String> {
        self.inner.query(state, seat, name, args)
    }
    async fn on_match_start(&self, seats: &[SeatInfo], settings: &Value, llm: &dyn LlmProvider) -> anyhow::Result<Value> {
        let mut state = self.inner.on_match_start(seats, settings, llm).await?;
        let run = Self::run();
        for s in seats.iter().filter(|s| !s.is_bot) {
            if let SeatAuth::Account(account) = self.accounts.seat_auth(&s.display_name, run.as_deref()) {
                let p = self.accounts.seat_persona(&account, s.seat);
                if let Some(next) = persona::apply_to_state(&state, &p, 0) {
                    state = next;
                }
            }
        }
        Ok(state)
    }
    async fn on_round_start(&self, state: &Value, round: u32, llm: &dyn LlmProvider) -> anyhow::Result<RoundStart> {
        self.inner.on_round_start(state, round, llm).await
    }
    async fn brief(&self, state: &Value, round: u32, llm: &dyn LlmProvider) -> anyhow::Result<Brief> {
        self.inner.brief(state, round, llm).await
    }
    fn validate_action(&self, state: &Value, seat: Seat, text: &str, fields: &Value, kind: SubmitKind) -> Result<(), String> {
        self.inner.validate_action(state, seat, text, fields, kind)
    }
    fn on_seat_takeover(&self, state: &Value, takeover: &SeatTakeover) -> Option<Value> {
        let run = Self::run();
        match self.accounts.seat_auth(&takeover.display_name, run.as_deref()) {
            SeatAuth::Impostor(_) if self.accounts.guard_on() => {
                theatre_trace::event(
                    persona::REFUSED_EVENT,
                    Some(&format!("seat:{}", takeover.seat)),
                    json!({ "round": takeover.round, "seat": takeover.seat,
                            "reason": "the name belongs to an account and this player is not signed in to it" }),
                );
                None
            }
            SeatAuth::Account(account) => {
                let base = self.inner.on_seat_takeover(state, takeover);
                let p = self.accounts.seat_persona(&account, takeover.seat);
                persona::apply_to_state(base.as_ref().unwrap_or(state), &p, takeover.round).or(base)
            }
            _ => self.inner.on_seat_takeover(state, takeover),
        }
    }
    fn default_action(&self, state: &Value, seat: Seat) -> Action {
        self.inner.default_action(state, seat)
    }
    fn on_action_begin(&self, state: &Value, round: u32, seat: Seat, window: Option<&WindowInfo>, next_window_id: u64) -> PhaseControl {
        self.inner.on_action_begin(state, round, seat, window, next_window_id)
    }
    fn on_instant(&self, state: &Value, seat: Seat, action: &Action) -> Option<Value> {
        self.inner.on_instant(state, seat, action)
    }
    async fn bot_action(&self, state: &Value, seat: Seat, llm: &dyn LlmProvider) -> anyhow::Result<Action> {
        self.inner.bot_action(state, seat, llm).await
    }
    async fn resolve_round(
        &self,
        state: &Value,
        round: u32,
        actions: &[Action],
        ctx: &RoundContext,
        llm: &dyn LlmProvider,
        progress: &dyn Progress,
    ) -> anyhow::Result<RoundOutcome> {
        self.inner.resolve_round(state, round, actions, ctx, llm, progress).await
    }
    fn fallback_outcome(&self, state: &Value, round: u32, actions: &[Action], ctx: &RoundContext) -> RoundOutcome {
        self.inner.fallback_outcome(state, round, actions, ctx)
    }
    fn default_choice(&self, state: &Value, seat: Seat, offered: &[Choice]) -> String {
        self.inner.default_choice(state, seat, offered)
    }
    async fn commit_round(&self, state: &Value, round: u32, picked: &[Picked], llm: &dyn LlmProvider) -> anyhow::Result<Committed> {
        self.inner.commit_round(state, round, picked, llm).await
    }
    fn is_match_over(&self, state: &Value, round: u32) -> bool {
        self.inner.is_match_over(state, round)
    }
    fn public_view(&self, state: &Value) -> Value {
        self.inner.public_view(state)
    }
    fn seat_view(&self, state: &Value, seat: Seat) -> Value {
        self.inner.seat_view(state, seat)
    }
    async fn summarize(&self, state: &Value, llm: &dyn LlmProvider) -> anyhow::Result<Summary> {
        self.inner.summarize(state, llm).await
    }
}
