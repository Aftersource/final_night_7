//! Drive a whole match through the `GamePlugin` trait exactly as the Theatre core does (Start → Brief → Compose →
//! Resolve → Commit until `is_match_over`, then `summarize`), in-process. Used by `fn7-sim` and the tests.

use serde_json::Value;
use theatre_plugin_api::*;

/// One beat as the core saw it.
pub struct BeatReport {
    pub round: u32,
    pub start: RoundStart,
    pub outcome: RoundOutcome,
    pub committed: Committed,
    /// The actions the core passed to `resolve_round`.
    pub actions: Vec<Action>,
}

/// Actions for seats a script controls (`None` = the core's behavior: `bot_action` for bots, `default_action` else).
pub type Script<'a> = dyn Fn(Seat, u32) -> Option<Action> + Send + Sync + 'a;

/// `on_beat` runs after each commit with the committed state; returning false stops the match there (it is still
/// summarized).
pub async fn run_match(
    p: &dyn GamePlugin,
    seats: &[SeatInfo],
    settings: &Value,
    llm: &dyn LlmProvider,
    stop_after: Option<u32>,
    script: &Script<'_>,
    mut on_beat: impl FnMut(&BeatReport, &Value) -> bool,
) -> anyhow::Result<(Value, Summary)> {
    let settings = p.validate_settings(settings).map_err(anyhow::Error::msg)?;
    let mut state = p.on_match_start(seats, &settings, llm).await?;
    let mut out: Vec<Seat> = vec![];
    let mut round = 0;
    loop {
        round += 1;
        let start = p.on_round_start(&state, round, llm).await?;
        state = start.state.clone();
        out.extend(start.eliminated.iter().map(|e| e.0));
        if p.is_match_over(&state, round - 1) {
            break;
        }
        let _brief = p.brief(&state, round, llm).await?;
        let mut actions = vec![];
        for s in seats.iter().filter(|s| !out.contains(&s.seat)) {
            let a = match script(s.seat, round) {
                Some(a) => a,
                None if s.is_bot => p.bot_action(&state, s.seat, llm).await?,
                None => p.default_action(&state, s.seat),
            };
            match p.validate_action(&state, s.seat, &a.text, &a.fields) {
                Ok(()) => actions.push(a),
                Err(_) => actions.push(p.default_action(&state, s.seat)),
            }
        }
        let outcome = match p
            .resolve_round(&state, round, &actions, llm, &NoProgress)
            .await
        {
            Ok(o) => o,
            Err(_) => p.fallback_outcome(&state, round, &actions),
        };
        state = outcome.state.clone();
        let committed = p.commit_round(&state, round, &[], llm).await?;
        state = committed.state.clone();
        out.extend(committed.eliminated.iter().map(|e| e.0));
        let rep = BeatReport {
            round,
            start,
            outcome,
            committed,
            actions,
        };
        let go_on = on_beat(&rep, &state);
        if !go_on || stop_after.is_some_and(|n| round >= n) || p.is_match_over(&state, round) {
            break;
        }
    }
    let summary = p.summarize(&state, llm).await?;
    Ok((state, summary))
}
